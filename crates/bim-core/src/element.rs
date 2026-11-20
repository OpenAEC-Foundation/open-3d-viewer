//! The five base elements from which BIM objects are built, as in Open BIM Studio's
//! `elements/` and `pattern/` modules:
//!
//! - **point** ([`PointDef`]): a type placed at one point;
//! - **linear** ([`LinearDef`]): a profile along a line (beam, column, member);
//! - **planar** ([`PlanarDef`]): a wall by its axis or a slab by its outline, with openings;
//! - **system** ([`SystemDef`]): a recipe on a domain that generates members and panels;
//! - **connection** ([`ConnectionDef`]): where two linear elements meet.
//!
//! An element holds its definition only. Its coordinates are f64 metres in the frame of its tile
//! ([`Element::tile`]), the tile that holds its placement origin. Solids are evaluated from the
//! definition ([`crate::Model::evaluate`]).

use crate::profile::{Profile, ProfileDef};
use crate::solid::{Axes, Plane, Solid};
use crate::tile::{TileGrid, TileKey};
use crate::{positive, Refusal, LINEAR_TOLERANCE};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

/// A stable element id: never reused within a model, the key of every reference (ADR-0011).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ElementId(pub u64);

/// The five base elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BaseKind {
    Point,
    Linear,
    Planar,
    System,
    Connection,
}

impl BaseKind {
    pub const ALL: [BaseKind; 5] = [BaseKind::Point, BaseKind::Linear, BaseKind::Planar, BaseKind::System, BaseKind::Connection];

    /// Its English name.
    pub fn name(self) -> &'static str {
        match self {
            BaseKind::Point => "Point",
            BaseKind::Linear => "Linear",
            BaseKind::Planar => "Planar",
            BaseKind::System => "System",
            BaseKind::Connection => "Connection",
        }
    }
}

/// A point element: a box of `size` (width along its x, depth, height) standing on `at`, turned
/// `rotation` radians about the vertical. With `opening` it is a door or a window: `at` is the
/// middle of its width at its foot, and it cuts an opening of its width and height in every wall
/// or framed system it stands in (see `Model::standing_openings`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointDef {
    pub at: DVec3,
    #[serde(default)]
    pub rotation: f64,
    pub size: DVec3,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opening: Option<OpeningKind>,
}

/// What a point element that cuts an opening is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningKind {
    Door,
    Window,
}

/// A linear element: `profile` from `start` to `end`, turned `roll` radians about its axis. A
/// horizontal member has its profile's y upwards; a vertical one its profile's x along the
/// project's x.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinearDef {
    pub start: DVec3,
    pub end: DVec3,
    pub profile: ProfileDef,
    #[serde(default)]
    pub roll: f64,
}

/// A rectangular opening in a wall: `along` the axis from its start to the opening's near side,
/// `sill` above the wall's base, `width` × `height`. An opening with no sill (a door) cuts the
/// wall's bottom edge; any other keeps wall below, above and on both sides.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Opening {
    pub along: f64,
    pub sill: f64,
    pub width: f64,
    pub height: f64,
    /// In a framed system: how this opening is framed where it is not as its system's type frames
    /// openings. A wall's opening has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framing: Option<OpeningFraming>,
}

/// The framing of one opening where it departs from its system's type: a header of its own
/// height, and more king studs or trimmers either side than one. What it leaves out is the type's.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct OpeningFraming {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kings: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trimmers: Option<u8>,
    /// In a lying system (a floor): the opening is round, drilled, its `width` the diameter.
    #[serde(default, skip_serializing_if = "is_false")]
    pub round: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Opening {
    /// How many king studs and how many trimmers stand either side of it in a frame: one of each,
    /// unless its own framing says two.
    pub fn studs_beside(&self) -> (u8, u8) {
        let own = self.framing.unwrap_or_default();
        (own.kings.unwrap_or(1).clamp(1, 2), own.trimmers.unwrap_or(1).clamp(1, 2))
    }
}

/// How a slab rises: from edge `edge` of its outline (the eave, where its top is at the slab's
/// `top`) inwards, at `angle` radians to the level. Its thickness is measured square to it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Slope {
    pub edge: usize,
    pub angle: f64,
}

/// The steepest a slab rises (radians): 85°.
pub const STEEPEST: f64 = 1.4835298641951802;

/// The form of a planar element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanarForm {
    /// A wall standing on its axis from `start` to `end` at height `base`, `height` high, centred
    /// on the axis.
    Wall { start: DVec2, end: DVec2, base: f64, height: f64 },
    /// A slab under `top`, of `outline` with `holes`.
    Slab {
        outline: Vec<DVec2>,
        #[serde(default)]
        holes: Vec<Vec<DVec2>>,
        top: f64,
        /// How far each edge of the outline bows: entry `i` is the bulge of the edge from corner
        /// `i` to the next (the tangent of a quarter of the arc's angle, above zero where the
        /// edge bows to the right of its way). Empty, or 0 for an edge: straight.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        bulges: Vec<f64>,
        /// What the slab is: a floor, a roof or a ceiling. A floor is not written.
        #[serde(default, skip_serializing_if = "SlabRole::is_floor")]
        role: SlabRole,
        /// How it rises from one of its edges (a roof's plane). None: it lies level.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slope: Option<Slope>,
    },
}

/// What a slab is in the building: it is made the same, and classed and listed by this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlabRole {
    #[default]
    Floor,
    Roof,
    Ceiling,
}

impl SlabRole {
    pub fn is_floor(&self) -> bool {
        *self == SlabRole::Floor
    }
}

/// A bulge this small is a straight edge.
const STRAIGHT: f64 = 1e-9;
/// An arc is made of chords that each turn at most this far (radians).
const CHORD_TURN: f64 = std::f64::consts::PI / 16.0;

/// The middle and the radius of the arc from `a` to `b` with `bulge`, the angle it starts at and
/// the angle it turns through (above zero: counter-clockwise). `None` where it is straight.
pub fn arc_of(a: DVec2, b: DVec2, bulge: f64) -> Option<(DVec2, f64, f64, f64)> {
    let chord = b - a;
    let length = chord.length();
    if bulge.abs() < STRAIGHT || !positive(length) {
        return None;
    }
    let left = DVec2::new(-chord.y, chord.x) / length;
    let centre = (a + b) / 2.0 + left * (length / 2.0 * (1.0 - bulge * bulge) / (2.0 * bulge));
    let from = a - centre;
    Some((centre, from.length(), from.y.atan2(from.x), 4.0 * bulge.atan()))
}

/// The points of the edge from `a` to `b` with `bulge`, `a` first and without `b`: `a` alone where
/// it is straight, else the arc in chords.
pub fn arc_points(a: DVec2, b: DVec2, bulge: f64) -> Vec<DVec2> {
    let Some((centre, radius, start, turn)) = arc_of(a, b, bulge) else { return vec![a] };
    let n = ((turn.abs() / CHORD_TURN).ceil() as usize).max(2);
    std::iter::once(a).chain((1..n).map(|k| centre + radius * DVec2::from_angle(start + turn * k as f64 / n as f64))).collect()
}

/// `outline` with its bowed edges in chords: the ring a solid is made of.
pub fn bowed(outline: &[DVec2], bulges: &[f64]) -> Vec<DVec2> {
    let n = outline.len();
    (0..n).flat_map(|i| arc_points(outline[i], outline[(i + 1) % n], bulges.get(i).copied().unwrap_or(0.0))).collect()
}

/// A planar element: a wall or a slab, `thickness` thick, with openings (walls only). With a
/// build-up, it is made of the build-up's layers and is as thick as they are together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanarDef {
    pub form: PlanarForm,
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openings: Vec<Opening>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_up: Option<BuildUpDef>,
}

/// A layer of a planar element: a material (a key of the project's library) and a thickness. A
/// cavity (an air layer) takes up its room but has no solid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerDef {
    pub material: String,
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cavity: bool,
}

/// A planar element's build-up: its type (the id of a build-up in the project's library) and the
/// type's layers, copied, so the element is whole without the library. A wall's layers run from
/// its exterior face in, its exterior on the right walking from start to end (as in Open BIM
/// Studio); a slab's from its top down.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildUpDef {
    pub id: String,
    pub layers: Vec<LayerDef>,
}

impl BuildUpDef {
    /// The build-up of the library's build-up `id`; a layer of a material hatched as air is a
    /// cavity. `None` when the library has no such build-up.
    pub fn from_library(library: &bim_library::Library, id: &str) -> Option<Self> {
        let b = library.buildup(id)?;
        let layers = b
            .layers
            .iter()
            .map(|l| LayerDef {
                material: l.material.clone(),
                thickness: l.thickness,
                cavity: library.material(&l.material).is_some_and(|m| m.hatch == bim_library::HatchKind::Air),
            })
            .collect();
        Some(Self { id: b.id.clone(), layers })
    }

    /// The layers together.
    pub fn thickness(&self) -> f64 {
        self.layers.iter().map(|l| l.thickness).sum()
    }

    /// Checks the layers: each thicker than the linear tolerance, and at least one not a cavity.
    pub fn check(&self) -> Result<(), Refusal> {
        if self.layers.iter().any(|l| !positive(l.thickness)) {
            return Err(Refusal::NotPositive);
        }
        if self.layers.iter().all(|l| l.cavity) {
            return Err(Refusal::NoLayers);
        }
        Ok(())
    }
}

impl PlanarDef {
    /// How thick it is: its build-up's layers together, else its own thickness.
    pub fn total_thickness(&self) -> f64 {
        self.build_up.as_ref().map_or(self.thickness, BuildUpDef::thickness)
    }

    /// It with build-up `build_up` (or none): its thickness becomes the build-up's.
    pub fn with_build_up(mut self, build_up: Option<BuildUpDef>) -> Self {
        if let Some(b) = &build_up {
            self.thickness = b.thickness();
        }
        self.build_up = build_up;
        self
    }
}

/// A system: a vertical domain on its line from `start` to `end`, from `base` up `height`,
/// divided into equal bays about `spacing` (along, up) apart. Members of `member` (width in the
/// face, depth across it) run on the division lines; panels `panel` thick fill the bays. A type
/// (`system_type`, copied from a library) adds a running bond, the materials and layers across the
/// face ([`crate::system`]). A framed type (a timber frame) frames `openings` in the face, measured
/// as a wall's from the system's start and base; a system that is not framed has none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemDef {
    pub start: DVec2,
    pub end: DVec2,
    pub base: f64,
    pub height: f64,
    pub spacing: DVec2,
    pub member: DVec2,
    pub panel: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_type: Option<crate::system::SystemTypeDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openings: Vec<Opening>,
    /// A framed system made in prefab units: where the user wants a joint between two units,
    /// along its line from its start. These come first; its type splits what lies between them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breaks: Vec<f64>,
    /// A lying system (a floor, a ceiling) laid in an outline: the outline in plan. It is laid
    /// along its line over the rectangle that holds the outline and cut to it; its openings are
    /// measured from its line's start. Empty: the rectangle its line and its `height` make.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outline: Vec<DVec2>,
}

/// How a connection joins its members.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKind {
    /// The secondary member stops at the face of the primary one, which runs through.
    Butt,
    /// Both members are cut by the plane that halves the angle between them.
    Mitre,
    /// The primary element cuts its body out of the secondary one (a beam through a wall, a duct
    /// through a slab). Any two elements with a body; the secondary keeps what is left.
    Cut,
    /// A steel end plate: the secondary stops a plate's thickness short of the primary's face,
    /// and a plate against that face is bolted to the primary (see [`crate::join::EndPlate`]).
    EndPlate(crate::join::EndPlate),
    /// A steel fin plate: the secondary stops a gap short of the primary's face, and a plate
    /// welded to that face is bolted through the secondary's web (see [`crate::join::FinPlate`]).
    FinPlate(crate::join::FinPlate),
    /// A steel splice: two members in one line each stop a plate's thickness short of where they
    /// meet, and the plates on their ends are bolted together (sizes as [`crate::join::EndPlate`]).
    Splice(crate::join::EndPlate),
    /// A steel base plate: the secondary (a column) stands on the primary on a plate, a bed of
    /// grout and four anchors (see [`crate::join::BasePlate`]).
    BasePlate(crate::join::BasePlate),
    /// The two are not joined where they meet: two building systems that would meet at a corner
    /// or a T junction are each made as they are alone, and two linear members get no join.
    Apart,
}

/// What a connection is to the analysis model: whether it passes a bending moment on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixity {
    /// A hinge: it passes forces on, no moment.
    Pinned,
    /// It passes moments on.
    Rigid,
}

impl ConnectionKind {
    /// What a steel connection is to the analysis model: a fin plate is a hinge, an end plate
    /// and a splice pass moments on, and a base plate with its four anchors is taken as a hinge. `None` for a join that is no connection of its own (a butt, a mitre,
    /// a cut).
    pub fn fixity(&self) -> Option<Fixity> {
        match self {
            ConnectionKind::FinPlate(_) | ConnectionKind::BasePlate(_) => Some(Fixity::Pinned),
            ConnectionKind::EndPlate(_) | ConnectionKind::Splice(_) => Some(Fixity::Rigid),
            _ => None,
        }
    }
}

/// A connection between two elements: two linear ones joined (butt, mitre), any two with a body
/// where one cuts the other, or two framed building systems where they meet at an edge (a corner
/// or a T junction): butt, the primary running through at a corner, or apart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionDef {
    pub kind: ConnectionKind,
    pub primary: ElementId,
    pub secondary: ElementId,
}

/// An element's definition, by base element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    Point(PointDef),
    Linear(LinearDef),
    Planar(PlanarDef),
    System(SystemDef),
    Connection(ConnectionDef),
}

/// Where the top of an element reaches: a level (its IfcX path) and how far above it (metres;
/// below it when negative). A wall's or a standing system's height, or a column's top end, is
/// worked out from it whenever the level moves; with none the element keeps its own height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Top {
    pub level: String,
    #[serde(default)]
    pub offset: f64,
}

/// An element: its id, name, tile, level and definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: ElementId,
    pub name: String,
    /// The tile whose frame its coordinates are in: the one holding its placement origin.
    pub tile: TileKey,
    /// The level it is placed on (the level's IfcX path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    /// The level its top reaches, if it reaches one (see [`Top`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<Top>,
    /// The strength class of what it is made of, when it is given: a steel grade (S235), a
    /// concrete class (C30/37), a timber class (C24).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<String>,
    /// The reinforcement of a concrete member with a rectangular section, when it is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reinforcement: Option<crate::reinforcement::Reinforcement>,
    pub shape: Shape,
}

impl Element {
    /// An element whose `shape` is given in project coordinates: it is placed in the tile that
    /// holds its placement origin, its coordinates turned into that tile's frame. Its id is set
    /// when it is added to a model.
    pub fn in_project(grid: &TileGrid, name: impl Into<String>, level: Option<String>, mut shape: Shape) -> Self {
        let tile = shape.placement().map_or(TileKey::default(), |p| grid.tile_of(p));
        shape.translate(-grid.origin(tile));
        Self { id: ElementId(0), name: name.into(), tile, level, top: None, grade: None, reinforcement: None, shape }
    }

    /// The same element (its id, name, level, top and grade) with `shape`, given in project coordinates.
    pub fn reshaped(&self, grid: &TileGrid, shape: Shape) -> Self {
        Self { id: self.id, top: self.top.clone(), grade: self.grade.clone(), reinforcement: self.reinforcement, ..Self::in_project(grid, self.name.clone(), self.level.clone(), shape) }
    }

    /// Its definition in project coordinates.
    pub fn shape_in_project(&self, grid: &TileGrid) -> Shape {
        let mut shape = self.shape.clone();
        shape.translate(grid.origin(self.tile));
        shape
    }

    /// Its base element.
    pub fn base(&self) -> BaseKind {
        self.shape.base()
    }

    /// Its IFC class (IFC4.3 names, ADR-0010).
    pub fn class(&self) -> &'static str {
        match &self.shape {
            Shape::Point(PointDef { opening: Some(OpeningKind::Door), .. }) => "IfcDoor",
            Shape::Point(PointDef { opening: Some(OpeningKind::Window), .. }) => "IfcWindow",
            Shape::Point(_) => "IfcBuildingElementProxy",
            Shape::Linear(l) => linear_class(l.end - l.start),
            Shape::Planar(PlanarDef { form: PlanarForm::Wall { .. }, .. }) => "IfcWall",
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { role: SlabRole::Roof, .. }, .. }) => "IfcRoof",
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { role: SlabRole::Ceiling, .. }, .. }) => "IfcCovering",
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { .. }, .. }) => "IfcSlab",
            Shape::System(_) => "IfcCurtainWall",
            // a join with parts of its own is an assembly of them; one without is a relation only
            Shape::Connection(ConnectionDef { kind: ConnectionKind::EndPlate(_) | ConnectionKind::FinPlate(_) | ConnectionKind::Splice(_) | ConnectionKind::BasePlate(_), .. }) => "IfcElementAssembly",
            Shape::Connection(_) => "IfcRelConnectsElements",
        }
    }

    /// Moves it by `delta` (metres). When its placement origin leaves its tile, it moves to the
    /// tile that holds it now, its coordinates carried over exactly.
    pub fn translate(&mut self, grid: &TileGrid, delta: DVec3) {
        self.shape.translate(delta);
        self.rehome(grid);
    }

    /// Puts it in the tile that holds its placement origin (ADR-0016), if it is not there.
    pub fn rehome(&mut self, grid: &TileGrid) {
        let Some(placement) = self.shape.placement() else { return };
        let home = grid.tile_of(grid.origin(self.tile) + placement);
        if home != self.tile {
            self.shape.translate(grid.offset(self.tile, home));
            self.tile = home;
        }
    }
}

impl Shape {
    /// Its base element.
    pub fn base(&self) -> BaseKind {
        match self {
            Shape::Point(_) => BaseKind::Point,
            Shape::Linear(_) => BaseKind::Linear,
            Shape::Planar(_) => BaseKind::Planar,
            Shape::System(_) => BaseKind::System,
            Shape::Connection(_) => BaseKind::Connection,
        }
    }

    /// Its placement origin; a connection has none (it lives in its primary member's tile).
    pub fn placement(&self) -> Option<DVec3> {
        match self {
            Shape::Point(p) => Some(p.at),
            Shape::Linear(l) => Some(l.start),
            Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, base, .. }, .. }) => Some(start.extend(*base)),
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, top, .. }, .. }) => outline.first().map(|p| p.extend(*top)),
            Shape::System(s) => Some(s.start.extend(s.base)),
            Shape::Connection(_) => None,
        }
    }

    /// Moves every coordinate by `delta`.
    pub fn translate(&mut self, delta: DVec3) {
        let flat = delta.truncate();
        match self {
            Shape::Point(p) => p.at += delta,
            Shape::Linear(l) => {
                l.start += delta;
                l.end += delta;
            }
            Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, base, .. }, .. }) => {
                *start += flat;
                *end += flat;
                *base += delta.z;
            }
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes, top, .. }, .. }) => {
                outline.iter_mut().chain(holes.iter_mut().flatten()).for_each(|p| *p += flat);
                *top += delta.z;
            }
            Shape::System(s) => {
                s.start += flat;
                s.end += flat;
                s.outline.iter_mut().for_each(|p| *p += flat);
                s.base += delta.z;
            }
            Shape::Connection(_) => {}
        }
    }
}

/// The IFC class of a linear element running along `axis`: a column within 1° of the vertical, a
/// beam within 1° of the horizontal, otherwise a member.
pub fn linear_class(axis: DVec3) -> &'static str {
    let Some(a) = axis.try_normalize() else { return "IfcMember" };
    let sin_1 = 1f64.to_radians().sin();
    if a.z.abs() >= 1f64.to_radians().cos() {
        "IfcColumn"
    } else if a.z.abs() <= sin_1 {
        "IfcBeam"
    } else {
        "IfcMember"
    }
}

/// A solid of an element and the IFC class of what it is (a curtain wall's members and panels have
/// classes of their own), and what it is made of when that is known (a material key of the
/// project's library).
#[derive(Debug, Clone, PartialEq)]
pub struct PartSolid {
    pub class: &'static str,
    pub solid: Solid,
    pub material: Option<String>,
}

/// The frame of a linear element: at its start, `z` along it, the profile's `x` square to it
/// (horizontal, or the project's x for a vertical member), turned by `roll`; and its length.
pub fn linear_axes(l: &LinearDef) -> Result<(Axes, f64), Refusal> {
    let axis = l.end - l.start;
    let length = axis.length();
    if !positive(length) {
        return Err(Refusal::ZeroLength);
    }
    let z = axis / length;
    let x = if z.z.abs() > 1.0 - 1e-12 { DVec3::X } else { DVec3::Z.cross(z).normalize() };
    let (sin, cos) = l.roll.sin_cos();
    let x = x * cos + z.cross(x) * sin;
    Ok((Axes::new(l.start, x, z)?, length))
}

/// The planes across a linear element at its two ends.
pub fn square_ends(axes: &Axes, length: f64) -> (Plane, Plane) {
    (Plane { point: axes.origin, normal: -axes.z }, Plane { point: axes.origin + axes.z * length, normal: axes.z })
}

/// The solid of a linear element between end planes (its own square ends where `None`).
pub fn linear_solid(l: &LinearDef, start: Option<Plane>, end: Option<Plane>) -> Result<Solid, Refusal> {
    let (axes, length) = linear_axes(l)?;
    let (s, e) = square_ends(&axes, length);
    Solid::sweep_between(&l.profile.profile()?, &axes, start.unwrap_or(s), end.unwrap_or(e))
}

/// The solid of a point element.
pub fn point_solid(p: &PointDef) -> Result<Solid, Refusal> {
    if !positive(p.size.z) {
        return Err(Refusal::NotPositive);
    }
    let (sin, cos) = p.rotation.sin_cos();
    let axes = Axes::new(p.at, DVec3::new(cos, sin, 0.0), DVec3::Z)?;
    Solid::extrude(&Profile::rectangle(p.size.x, p.size.y)?, &axes, p.size.z)
}

/// How wide the members of a door or window frame are, how deep at least, and how thick a pane.
pub const FRAME_WIDTH: f64 = 0.06;
pub const FRAME_DEPTH: f64 = 0.09;
pub const PANE: f64 = 0.02;

/// The solids of a door or a window: its frame (a door has none under its leaf) and what fills
/// it, a leaf or a pane of glass (marked true). One too small for a frame is a single block.
pub fn opening_solids(p: &PointDef) -> Result<Vec<(Solid, bool)>, Refusal> {
    let (w, h, f) = (p.size.x, p.size.z, FRAME_WIDTH);
    let Some(kind) = p.opening.filter(|_| w > 3.0 * f && h > 3.0 * f) else { return Ok(vec![(point_solid(p)?, false)]) };
    let depth = p.size.y.max(FRAME_DEPTH);
    let (sin, cos) = p.rotation.sin_cos();
    let along = DVec3::new(cos, sin, 0.0);
    // a block: its middle along the width, its foot above the sill, and its sizes
    let block = |x: f64, z: f64, size: DVec3| point_solid(&PointDef { at: p.at + along * x + DVec3::Z * z, rotation: p.rotation, size, opening: None });
    let side = w / 2.0 - f / 2.0;
    let mut solids = Vec::new();
    match kind {
        OpeningKind::Window => {
            solids.push((block(0.0, 0.0, DVec3::new(w, depth, f))?, false));
            solids.push((block(0.0, h - f, DVec3::new(w, depth, f))?, false));
            for x in [-side, side] {
                solids.push((block(x, f, DVec3::new(f, depth, h - 2.0 * f))?, false));
            }
            solids.push((block(0.0, f, DVec3::new(w - 2.0 * f, PANE, h - 2.0 * f))?, true));
        }
        OpeningKind::Door => {
            solids.push((block(0.0, h - f, DVec3::new(w, depth, f))?, false));
            for x in [-side, side] {
                solids.push((block(x, 0.0, DVec3::new(f, depth, h - f))?, false));
            }
            solids.push((block(0.0, 0.0, DVec3::new(w - 2.0 * f, p.size.y.min(depth), h - f))?, false));
        }
    }
    Ok(solids)
}

/// The solids of a planar element, each through all its layers: one, or one per piece of a wall
/// that openings through its whole height (or width) split.
pub fn planar_solids(p: &PlanarDef) -> Result<Vec<Solid>, Refusal> {
    let thickness = p.total_thickness();
    if !positive(thickness) {
        return Err(Refusal::NotPositive);
    }
    if let Some(b) = &p.build_up {
        b.check()?;
    }
    let (profiles, axes) = planar_base(p, thickness, [0.0; 2])?;
    profiles.iter().map(|profile| Solid::extrude(profile, &axes, thickness)).collect()
}

/// The solids of a planar element with their materials: one per layer of its build-up (none for a
/// cavity), or its one solid without a material.
pub fn planar_parts(p: &PlanarDef) -> Result<Vec<(Option<String>, Solid)>, Refusal> {
    planar_parts_extended(p, [0.0; 2])
}

/// The solids of a planar element as [`planar_parts`] makes them, a wall reaching `extend` metres
/// past its start and its end (to be cut back to its corners, see `joins`); a slab as it is.
pub fn planar_parts_extended(p: &PlanarDef, extend: [f64; 2]) -> Result<Vec<(Option<String>, Solid)>, Refusal> {
    let Some(b) = &p.build_up else {
        let thickness = p.total_thickness();
        if !positive(thickness) {
            return Err(Refusal::NotPositive);
        }
        let (profiles, axes) = planar_base(p, thickness, extend)?;
        return profiles.iter().map(|profile| Ok((None, Solid::extrude(profile, &axes, thickness)?))).collect();
    };
    b.check()?;
    let thickness = b.thickness();
    let (profiles, axes) = planar_base(p, thickness, extend)?;
    let mut parts = Vec::new();
    let mut from = 0.0;
    for layer in &b.layers {
        let to = from + layer.thickness;
        if !layer.cavity {
            // a wall's layers from its exterior (its right face, at z = thickness) in; a slab's
            // from its top (at z = thickness) down
            let at = Axes::new(axes.origin + axes.z * (thickness - to), axes.x, axes.z)?;
            for profile in &profiles {
                parts.push((Some(layer.material.clone()), Solid::extrude(profile, &at, layer.thickness)?));
            }
        }
        from = to;
    }
    Ok(parts)
}

/// What a planar element `thickness` thick extrudes, and the frame it extrudes in along its z: a
/// wall's elevation from its left face (reaching `extend` past its start and end), a profile per
/// piece of wall; a slab's outline from its underside.
fn planar_base(p: &PlanarDef, thickness: f64, extend: [f64; 2]) -> Result<(Vec<Profile>, Axes), Refusal> {
    match &p.form {
        PlanarForm::Wall { start, end, base, height } => {
            let (axes, length) = wall_axes(*start, *end, *base, thickness)?;
            if !positive(*height) {
                return Err(Refusal::NotPositive);
            }
            // the openings must fit the wall itself, not only its reach past its ends
            let elevation = wall_elevation(length, *height, &p.openings)?;
            if extend == [0.0; 2] {
                return Ok((elevation, axes));
            }
            // an opening that reaches an end goes on through the reach past it
            let tol = LINEAR_TOLERANCE;
            let reach_length = length + extend[0] + extend[1];
            let shifted: Vec<Opening> = p
                .openings
                .iter()
                .map(|o| {
                    let from = if o.along <= tol { 0.0 } else { o.along + extend[0] };
                    let to = if o.along + o.width >= length - tol { reach_length } else { o.along + o.width + extend[0] };
                    Opening { along: from, width: to - from, ..*o }
                })
                .collect();
            let reach = Axes::new(axes.origin - axes.x * extend[0], axes.x, axes.z)?;
            Ok((wall_elevation(reach_length, *height, &shifted)?, reach))
        }
        PlanarForm::Slab { outline, holes, top, bulges, slope, .. } => {
            let ring = bowed(outline, bulges);
            let Some(s) = slope.filter(|s| s.angle.abs() > 1e-9 && outline.len() >= 3) else {
                let axes = Axes::new(DVec3::new(0.0, 0.0, top - thickness), DVec3::X, DVec3::Z)?;
                return Ok((vec![Profile::new(ring, holes.clone())?], axes));
            };
            if !(s.angle.abs() <= STEEPEST) {
                return Err(Refusal::NotPositive);
            }
            // the eave, and the way in from it: to the left of an outline that runs anticlockwise
            let n = outline.len();
            let (a, b) = (outline[s.edge % n], outline[(s.edge + 1) % n]);
            let along = (b - a).normalize_or_zero();
            if along == DVec2::ZERO {
                return Err(Refusal::ZeroLength);
            }
            let area: f64 = (0..n).map(|k| outline[k].perp_dot(outline[(k + 1) % n])).sum();
            let d = if area >= 0.0 { along.perp() } else { -along.perp() };
            // the plane it lies in: x up the slope, y level along the eave, z square to it
            let (sin, cos) = s.angle.sin_cos();
            let x = (d * cos).extend(sin);
            let normal = (-d * sin).extend(cos);
            let level = d.perp();
            let on = |q: &DVec2| DVec2::new((*q - a).dot(d) / cos, (*q - a).dot(level));
            let axes = Axes::new(a.extend(*top) - normal * thickness, x, normal)?;
            let profile = Profile::new(ring.iter().map(on).collect(), holes.iter().map(|h| h.iter().map(on).collect()).collect())?;
            Ok((vec![profile], axes))
        }
    }
}

/// The frame of a wall's elevation: at the axis start, `thickness / 2` to its left, at `base`;
/// `x` along the axis, `y` up, `z` across to the right. And the axis length.
fn wall_axes(start: DVec2, end: DVec2, base: f64, thickness: f64) -> Result<(Axes, f64), Refusal> {
    let d = end - start;
    let length = d.length();
    if !positive(length) {
        return Err(Refusal::ZeroLength);
    }
    let dir = (d / length).extend(0.0);
    let right = dir.cross(DVec3::Z);
    Ok((Axes::new(start.extend(base) - right * (thickness / 2.0), dir, right)?, length))
}

/// A wall's elevation, `length` × `height`, less its openings: a profile per piece of wall that is
/// left (from the start, bottom up), each outline with the holes in it.
///
/// An opening may reach the base (a door), the top or an end, and an opening through the wall's
/// whole height (or length) splits it; what lies past the wall's edges takes nothing. An opening
/// must take something from the wall, openings must not touch, and some wall must be left.
///
/// The elevation is cut on the grid of the wall's and the openings' edges (an edge within the
/// linear tolerance of another taken onto it): each cell is wall or opening, the cells of wall that
/// share a side are one piece, and a piece's outline and holes are the sides between wall and the
/// rest, run with the wall on their left.
fn wall_elevation(length: f64, height: f64, openings: &[Opening]) -> Result<Vec<Profile>, Refusal> {
    let tol = LINEAR_TOLERANCE;
    for o in openings {
        if !positive(o.width) || !positive(o.height) || !o.along.is_finite() || !o.sill.is_finite() {
            return Err(Refusal::NotPositive);
        }
        if o.along >= length - tol || o.along + o.width <= tol || o.sill >= height - tol || o.sill + o.height <= tol {
            return Err(Refusal::OpeningOutside);
        }
    }
    for (i, a) in openings.iter().enumerate() {
        for b in &openings[i + 1..] {
            let apart_along = a.along + a.width < b.along - tol || b.along + b.width < a.along - tol;
            let apart_up = a.sill + a.height < b.sill - tol || b.sill + b.height < a.sill - tol;
            if !apart_along && !apart_up {
                return Err(Refusal::OpeningsOverlap);
            }
        }
    }
    // the grid lines: the wall's edges and the openings' within it, near ones taken as one
    let lines = |ends: Vec<f64>, max: f64| -> Vec<f64> {
        let mut values: Vec<f64> = ends.into_iter().map(|v| v.clamp(0.0, max)).chain([0.0, max]).collect();
        values.sort_by(f64::total_cmp);
        let mut kept: Vec<f64> = Vec::new();
        for v in values {
            if kept.last().is_none_or(|k| v - k > tol) {
                kept.push(v);
            }
        }
        // the far edge is the wall's own
        if let Some(last) = kept.last_mut() {
            *last = max;
        }
        kept
    };
    let xs = lines(openings.iter().flat_map(|o| [o.along, o.along + o.width]).collect(), length);
    let ys = lines(openings.iter().flat_map(|o| [o.sill, o.sill + o.height]).collect(), height);
    let (nx, ny) = (xs.len() - 1, ys.len() - 1);
    // a cell is wall unless its middle lies in an opening
    let wall = |i: usize, j: usize| {
        let (x, y) = ((xs[i] + xs[i + 1]) / 2.0, (ys[j] + ys[j + 1]) / 2.0);
        !openings.iter().any(|o| o.along < x && x < o.along + o.width && o.sill < y && y < o.sill + o.height)
    };
    let solid: Vec<Vec<bool>> = (0..nx).map(|i| (0..ny).map(|j| wall(i, j)).collect()).collect();
    let mut piece = vec![vec![usize::MAX; ny]; nx];
    let mut pieces = Vec::new();
    for i0 in 0..nx {
        for j0 in 0..ny {
            if !solid[i0][j0] || piece[i0][j0] != usize::MAX {
                continue;
            }
            // the cells of this piece, and the sides it shares with the rest, wall on their left
            let k = pieces.len();
            // ordered, so every ring starts at its lowest corner and the result is the same every time
            let mut edges: std::collections::BTreeMap<(usize, usize), (usize, usize)> = std::collections::BTreeMap::new();
            let mut stack = vec![(i0, j0)];
            piece[i0][j0] = k;
            while let Some((i, j)) = stack.pop() {
                let neighbours = [(i.checked_sub(1), Some(j)), ((i + 1 < nx).then_some(i + 1), Some(j)), (Some(i), j.checked_sub(1)), (Some(i), (j + 1 < ny).then_some(j + 1))];
                for (a, b) in neighbours {
                    if let (Some(a), Some(b)) = (a, b) {
                        if solid[a][b] && piece[a][b] == usize::MAX {
                            piece[a][b] = k;
                            stack.push((a, b));
                        }
                    }
                }
                let open = |a: Option<usize>, b: Option<usize>| !matches!((a, b), (Some(a), Some(b)) if solid[a][b]);
                let sides = [
                    (open(Some(i), j.checked_sub(1)), (i, j), (i + 1, j)),
                    (open((i + 1 < nx).then_some(i + 1), Some(j)), (i + 1, j), (i + 1, j + 1)),
                    (open(Some(i), (j + 1 < ny).then_some(j + 1)), (i + 1, j + 1), (i, j + 1)),
                    (open(i.checked_sub(1), Some(j)), (i, j + 1), (i, j)),
                ];
                for (bounds, from, to) in sides {
                    // a corner two sides leave is a pinch, which only touching openings make
                    if bounds && edges.insert(from, to).is_some() {
                        return Err(Refusal::OpeningsOverlap);
                    }
                }
            }
            // the sides run into rings: the outline counter-clockwise, holes clockwise
            let mut outer = None;
            let mut holes = Vec::new();
            while let Some((&start, _)) = edges.first_key_value() {
                let mut ring = Vec::new();
                let mut at = start;
                while let Some(next) = edges.remove(&at) {
                    ring.push(DVec2::new(xs[at.0], ys[at.1]));
                    at = next;
                }
                let area: f64 = (0..ring.len()).map(|n| ring[n].perp_dot(ring[(n + 1) % ring.len()])).sum();
                if area > 0.0 {
                    outer = Some(ring);
                } else {
                    holes.push(ring);
                }
            }
            pieces.push(Profile::new(outer.ok_or(Refusal::Degenerate)?, holes)?);
        }
    }
    if pieces.is_empty() {
        return Err(Refusal::Empty);
    }
    Ok(pieces)
}

/// The members and panels of a system, by its type when it has one ([`crate::system`]).
pub fn system_parts(s: &SystemDef) -> Result<Vec<PartSolid>, Refusal> {
    crate::system::parts(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(openings: Vec<Opening>) -> PlanarDef {
        PlanarDef { form: PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(5.0, 0.0), base: 0.0, height: 3.0 }, thickness: 0.2, openings, build_up: None }
    }

    /// The solid of a planar element in one piece.
    fn planar_solid(p: &PlanarDef) -> Result<Solid, Refusal> {
        planar_solids(p).map(|mut pieces| {
            assert_eq!(pieces.len(), 1, "one piece");
            pieces.remove(0)
        })
    }

    fn at(along: f64, sill: f64, width: f64, height: f64) -> Opening {
        Opening { along, sill, width, height, framing: None }
    }

    /// The volume of a 5 × 3 m wall 0.2 thick with `area` square metres of its elevation taken out.
    fn less(area: f64) -> f64 {
        (5.0 * 3.0 - area) * 0.2
    }

    #[test]
    fn a_slab_edge_with_a_bulge_bows_and_is_read_back() {
        // a 2 × 2 square whose east edge is a half circle outward (it runs north, bowing right)
        let outline = vec![DVec2::ZERO, DVec2::new(2.0, 0.0), DVec2::new(2.0, 2.0), DVec2::new(0.0, 2.0)];
        let ring = bowed(&outline, &[0.0, 1.0]);
        assert_eq!(ring.len(), 3 + 16);
        assert!(ring.iter().any(|p| (*p - DVec2::new(3.0, 1.0)).length() < 1e-9), "the arc reaches a metre out: {ring:?}");
        let form = PlanarForm::Slab { outline, holes: Vec::new(), top: 3.0, bulges: vec![0.0, 1.0], role: Default::default(), slope: None };
        let slab = PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None };
        let solid = planar_solid(&slab).unwrap();
        let area = 4.0 + std::f64::consts::PI / 2.0;
        assert!((solid.volume() - 0.2 * area).abs() < 0.2 * 0.02, "{}", solid.volume());
        // written and read, the bulges are kept; a slab without them writes none
        let text = serde_json::to_string(&slab).unwrap();
        assert_eq!(serde_json::from_str::<PlanarDef>(&text).unwrap(), slab);
        let straight = PlanarDef { form: PlanarForm::Slab { outline: vec![DVec2::ZERO, DVec2::X, DVec2::Y], holes: Vec::new(), top: 0.0, bulges: Vec::new(), role: Default::default(), slope: None }, ..slab.clone() };
        assert!(!serde_json::to_string(&straight).unwrap().contains("bulges"));
    }

    #[test]
    fn a_wall_is_its_elevation_extruded_through_its_thickness() {
        let s = planar_solid(&wall(Vec::new())).unwrap();
        assert!((s.volume() - 5.0 * 3.0 * 0.2).abs() < 1e-12);
        let (lo, hi) = s.bounds();
        assert!((lo - DVec3::new(0.0, -0.1, 0.0)).length() < 1e-12 && (hi - DVec3::new(5.0, 0.1, 3.0)).length() < 1e-12, "{lo} {hi}");
    }

    #[test]
    fn openings_cut_the_wall_without_booleans() {
        let door = at(0.5, 0.0, 0.9, 2.1);
        let window = Opening { along: 2.0, sill: 0.9, width: 1.5, height: 1.2, framing: None };
        let s = planar_solid(&wall(vec![door, window])).unwrap();
        let expected = (5.0 * 3.0 - 0.9 * 2.1 - 1.5 * 1.2) * 0.2;
        assert!((s.volume() - expected).abs() < 1e-12, "{} vs {expected}", s.volume());
    }

    #[test]
    fn openings_that_take_nothing_touch_or_take_everything_are_refused() {
        assert_eq!(planar_solids(&wall(vec![at(5.5, 0.9, 1.0, 1.0)])), Err(Refusal::OpeningOutside), "past the end");
        assert_eq!(planar_solids(&wall(vec![at(1.0, 3.2, 1.0, 1.0)])), Err(Refusal::OpeningOutside), "above the top");
        assert_eq!(planar_solids(&wall(vec![at(-2.0, 0.9, 2.0, 1.0)])), Err(Refusal::OpeningOutside), "before the start");
        assert_eq!(planar_solids(&wall(vec![at(1.0, 0.9, 1.0, 1.0), at(1.5, 1.5, 1.0, 1.0)])), Err(Refusal::OpeningsOverlap));
        assert_eq!(planar_solids(&wall(vec![at(1.0, 0.9, 1.0, 1.0), at(2.0, 0.9, 1.0, 1.0)])), Err(Refusal::OpeningsOverlap), "touching");
        assert_eq!(planar_solids(&wall(vec![at(1.0, 0.9, 0.0, 1.0)])), Err(Refusal::NotPositive));
        assert_eq!(planar_solids(&wall(vec![at(-0.1, -0.1, 5.2, 3.2)])), Err(Refusal::Empty), "the whole wall");
    }

    #[test]
    fn an_opening_may_reach_the_top_or_an_end() {
        let volume = |o: Opening| planar_solid(&wall(vec![o])).unwrap().volume();
        let cases = [
            (at(0.0, 0.9, 1.0, 1.0), 1.0, "at the start"),
            (at(4.5, 0.9, 0.5, 1.0), 0.5, "at the end"),
            (at(1.0, 2.0, 1.0, 1.0), 1.0, "to the top"),
            (at(1.0, 2.5, 1.0, 1.0), 0.5, "past the top: what lies in the wall"),
            (at(1.0, -0.1, 1.0, 1.0), 0.9, "from below the base: a door"),
            (at(4.0, 2.0, 1.5, 1.5), 1.0, "in the top corner at the end"),
            (at(-0.3, 0.0, 1.3, 2.1), 2.1, "a door at the start, past it"),
        ];
        for (o, area, what) in cases {
            assert!((volume(o) - less(area)).abs() < 1e-12, "{what}: {} vs {}", volume(o), less(area));
        }
    }

    #[test]
    fn an_opening_through_the_whole_height_or_length_splits_the_wall() {
        // a door to the top: two pieces, from the start
        let pieces = planar_solids(&wall(vec![at(2.0, 0.0, 1.0, 3.0)])).unwrap();
        assert_eq!(pieces.len(), 2);
        let (lo, hi) = pieces[0].bounds();
        assert!(lo.x.abs() < 1e-12 && (hi.x - 2.0).abs() < 1e-12, "{lo} {hi}");
        let (lo, hi) = pieces[1].bounds();
        assert!((lo.x - 3.0).abs() < 1e-12 && (hi.x - 5.0).abs() < 1e-12, "{lo} {hi}");
        assert!(pieces.iter().all(|p| (p.volume() - 2.0 * 3.0 * 0.2).abs() < 1e-12));
        // a band through the whole length: below and above it, bottom up
        let pieces = planar_solids(&wall(vec![at(-0.5, 1.0, 6.0, 1.0)])).unwrap();
        assert_eq!(pieces.len(), 2);
        assert!((pieces[0].bounds().1.z - 1.0).abs() < 1e-12 && (pieces[1].bounds().0.z - 2.0).abs() < 1e-12);
        // with a window in one piece and a door to the top beside it
        let pieces = planar_solids(&wall(vec![at(0.5, 0.9, 1.0, 1.2), at(2.0, 0.0, 1.0, 3.0), at(3.5, 0.0, 0.9, 2.1)])).unwrap();
        let volume: f64 = pieces.iter().map(Solid::volume).sum();
        assert_eq!(pieces.len(), 2);
        assert!((volume - less(1.2 + 3.0 + 0.9 * 2.1)).abs() < 1e-12, "{volume}");
        // every layer of a layered wall is split alike
        let mut w = cavity_wall();
        w.openings = vec![at(2.0, 0.0, 1.0, 3.0)];
        let parts = planar_parts(&w).unwrap();
        assert_eq!(parts.len(), 3 * 2);
        let volume: f64 = parts.iter().map(|(_, s)| s.volume()).sum();
        assert!((volume - 4.0 * 3.0 * 0.32).abs() < 1e-9, "{volume}");
    }

    #[test]
    fn edges_within_the_tolerance_of_the_wall_or_each_other_are_taken_as_one() {
        // a window a micrometre from the start and one whose edge lies a nanometre beside the first's
        let near = 1e-7;
        let s = planar_solid(&wall(vec![at(near, 0.9, 1.0, 1.0), at(2.0, 0.9 + 1e-9, 1.0, 1.0)])).unwrap();
        // the first taken onto the start, so it is that much wider; the second onto the first's lines
        assert!((s.volume() - less(2.0 + near)).abs() < 1e-12, "{}", s.volume());
        assert_eq!(s.positions.len(), 2 * (8 + 4), "an outline of eight corners and a hole of four on either face: no slivers");
    }

    #[test]
    fn a_slab_hangs_under_its_top() {
        let outline = vec![DVec2::ZERO, DVec2::new(4.0, 0.0), DVec2::new(4.0, 3.0), DVec2::new(0.0, 3.0)];
        // rising at 45° from its first edge, its top goes up as far as it goes in, and it is as
        // much longer as the slope is
        let pitched = PlanarDef { form: PlanarForm::Slab { outline: outline.clone(), holes: Vec::new(), top: 3.0, bulges: Vec::new(), role: SlabRole::Roof, slope: Some(Slope { edge: 0, angle: std::f64::consts::FRAC_PI_4 }) }, thickness: 0.25, openings: Vec::new(), build_up: None };
        let roof = planar_solid(&pitched).unwrap();
        let level = planar_solid(&PlanarDef { form: PlanarForm::Slab { outline: outline.clone(), holes: Vec::new(), top: 3.0, bulges: Vec::new(), role: Default::default(), slope: None }, thickness: 0.25, openings: Vec::new(), build_up: None }).unwrap();
        assert!((roof.volume() - level.volume() * std::f64::consts::SQRT_2).abs() < 1e-9, "{} {}", roof.volume(), level.volume());
        let depth = outline.iter().map(|p| (*p - outline[0]).dot((outline[1] - outline[0]).normalize().perp()).abs()).fold(0.0, f64::max);
        let highest = roof.bounds().1.z;
        assert!((highest - (3.0 + depth)).abs() < 1e-9, "{highest} {depth}");
        let s = planar_solid(&PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top: 3.0, bulges: Vec::new(), role: Default::default(), slope: None }, thickness: 0.25, openings: Vec::new(), build_up: None }).unwrap();
        let (lo, hi) = s.bounds();
        assert_eq!((lo.z, hi.z), (2.75, 3.0));
        assert!((s.volume() - 4.0 * 3.0 * 0.25).abs() < 1e-12);
    }

    fn cavity_wall() -> PlanarDef {
        let layer = |material: &str, thickness, cavity| LayerDef { material: material.into(), thickness, cavity };
        let build_up = BuildUpDef {
            id: "wall.cavity-pir-360".into(),
            layers: vec![layer("brick", 0.1, false), layer("air", 0.04, true), layer("insulation-pir", 0.1, false), layer("sand-lime", 0.12, false)],
        };
        wall(Vec::new()).with_build_up(Some(build_up))
    }

    #[test]
    fn a_layered_wall_is_a_solid_per_layer_from_its_exterior_on_the_right() {
        let w = cavity_wall();
        assert!((w.thickness - 0.36).abs() < 1e-12, "the thickness follows the layers");
        let parts = planar_parts(&w).unwrap();
        let materials: Vec<&str> = parts.iter().map(|(m, _)| m.as_deref().unwrap()).collect();
        assert_eq!(materials, ["brick", "insulation-pir", "sand-lime"], "the cavity has no solid");
        // the wall runs along +x, so its right (exterior) side is -y; it is centred on its axis
        let (lo, hi) = parts[0].1.bounds();
        assert!((lo.y + 0.18).abs() < 1e-12 && (hi.y + 0.08).abs() < 1e-12, "the brick outside: {lo} {hi}");
        let (lo, hi) = parts[2].1.bounds();
        assert!((lo.y - 0.06).abs() < 1e-12 && (hi.y - 0.18).abs() < 1e-12, "the sand-lime inside: {lo} {hi}");
        let volume: f64 = parts.iter().map(|(_, s)| s.volume()).sum();
        assert!((volume - 5.0 * 3.0 * 0.32).abs() < 1e-9, "all but the 40 mm cavity: {volume}");
        assert!((planar_solid(&w).unwrap().volume() - 5.0 * 3.0 * 0.36).abs() < 1e-9, "one body through all layers");
    }

    #[test]
    fn openings_go_through_every_layer() {
        let mut w = cavity_wall();
        w.openings = vec![Opening { along: 1.0, sill: 0.9, width: 1.2, height: 1.2, framing: None }];
        let volume: f64 = planar_parts(&w).unwrap().iter().map(|(_, s)| s.volume()).sum();
        assert!((volume - (5.0 * 3.0 - 1.2 * 1.2) * 0.32).abs() < 1e-9, "{volume}");
    }

    #[test]
    fn a_layered_slab_has_its_first_layer_on_top() {
        let layer = |material: &str, thickness| LayerDef { material: material.into(), thickness, cavity: false };
        let outline = vec![DVec2::ZERO, DVec2::new(4.0, 0.0), DVec2::new(4.0, 3.0), DVec2::new(0.0, 3.0)];
        let slab = PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top: 3.0, bulges: Vec::new(), role: Default::default(), slope: None }, thickness: 0.0, openings: Vec::new(), build_up: None }
            .with_build_up(Some(BuildUpDef { id: "floor".into(), layers: vec![layer("screed", 0.05), layer("concrete", 0.2), layer("insulation", 0.1)] }));
        let parts = planar_parts(&slab).unwrap();
        let z = |i: usize| {
            let (lo, hi) = parts[i].1.bounds();
            (lo.z, hi.z)
        };
        let near = |(a, b): (f64, f64), (c, d): (f64, f64)| (a - c).abs() < 1e-12 && (b - d).abs() < 1e-12;
        assert!(near(z(0), (2.95, 3.0)), "the screed on top: {:?}", z(0));
        assert!(near(z(1), (2.75, 2.95)), "{:?}", z(1));
        assert!(near(z(2), (2.65, 2.75)), "{:?}", z(2));
    }

    #[test]
    fn a_build_up_of_cavities_or_empty_layers_is_refused() {
        let mut w = cavity_wall();
        w.build_up.as_mut().unwrap().layers.iter_mut().for_each(|l| l.cavity = true);
        assert_eq!(planar_parts(&w), Err(Refusal::NoLayers));
        assert_eq!(planar_solid(&w), Err(Refusal::NoLayers));
        let mut w = cavity_wall();
        w.build_up.as_mut().unwrap().layers[1].thickness = 0.0;
        assert_eq!(planar_parts(&w), Err(Refusal::NotPositive));
        let mut w = cavity_wall();
        w.build_up.as_mut().unwrap().layers.clear();
        assert_eq!(planar_parts(&w), Err(Refusal::NoLayers), "no layers at all");
    }

    #[test]
    fn a_build_up_comes_from_the_library_with_its_cavities() {
        let library = bim_library::Library::starter();
        let b = BuildUpDef::from_library(&library, "wall.cavity-pir-360").unwrap();
        assert_eq!(b, cavity_wall().build_up.unwrap());
        assert!(BuildUpDef::from_library(&library, "wall.nothing").is_none());
    }

    #[test]
    fn a_layered_definition_round_trips_and_an_old_one_still_reads() {
        let w = cavity_wall();
        let json = serde_json::to_string(&w).unwrap();
        assert!(json.contains("\"cavity\":true") && json.matches("cavity").count() == 2, "{json}");
        assert_eq!(serde_json::from_str::<PlanarDef>(&json).unwrap(), w);
        let old = r#"{"form":{"wall":{"start":[0.0,0.0],"end":[5.0,0.0],"base":0.0,"height":3.0}},"thickness":0.2}"#;
        assert_eq!(serde_json::from_str::<PlanarDef>(old).unwrap(), wall(Vec::new()));
    }

    #[test]
    fn linear_elements_have_their_class_by_direction() {
        assert_eq!(linear_class(DVec3::new(5.0, 0.0, 0.0)), "IfcBeam");
        assert_eq!(linear_class(DVec3::new(0.0, 0.0, 3.0)), "IfcColumn");
        assert_eq!(linear_class(DVec3::new(3.0, 0.0, 3.0)), "IfcMember");
    }

    #[test]
    fn a_beam_has_its_profile_upright() {
        let l = LinearDef { start: DVec3::ZERO, end: DVec3::new(6.0, 0.0, 0.0), profile: ProfileDef::Rectangle { width: 0.2, height: 0.4 }, roll: 0.0 };
        let s = linear_solid(&l, None, None).unwrap();
        let (lo, hi) = s.bounds();
        assert!((hi - lo - DVec3::new(6.0, 0.2, 0.4)).length() < 1e-12, "{}", hi - lo);
        let column = LinearDef { start: DVec3::ZERO, end: DVec3::new(0.0, 0.0, 3.0), profile: ProfileDef::Rectangle { width: 0.3, height: 0.5 }, roll: 0.0 };
        let (lo, hi) = linear_solid(&column, None, None).unwrap().bounds();
        assert!((hi - lo - DVec3::new(0.3, 0.5, 3.0)).length() < 1e-12, "{}", hi - lo);
    }

    #[test]
    fn a_system_fills_its_domain_with_members_and_panels() {
        let s = SystemDef { start: DVec2::ZERO, end: DVec2::new(3.0, 0.0), base: 0.0, height: 3.0, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.03, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new() };
        let parts = system_parts(&s).unwrap();
        let members = parts.iter().filter(|p| p.class == "IfcMember").count();
        let panels = parts.iter().filter(|p| p.class == "IfcPlate").count();
        // 3 mullions, 2 bays × 3 transoms, 2 × 2 panels
        assert_eq!((members, panels), (3 + 6, 4));
        let glass: f64 = parts.iter().filter(|p| p.class == "IfcPlate").map(|p| p.solid.volume()).sum();
        assert!((glass - 4.0 * 1.45 * 1.45 * 0.03).abs() < 1e-12);
        let tight = SystemDef { spacing: DVec2::new(0.04, 1.0), ..s };
        assert_eq!(system_parts(&tight), Err(Refusal::NotPositive));
    }

    #[test]
    fn an_element_lives_in_the_tile_of_its_placement() {
        let grid = TileGrid::default();
        let shape = Shape::Point(PointDef { at: DVec3::new(1500.25, -20.5, 1.0), rotation: 0.0, size: DVec3::ONE, opening: None });
        let mut e = Element::in_project(&grid, "Box", None, shape.clone());
        assert_eq!(e.tile, TileKey { x: 1, y: -1 });
        assert_eq!(e.shape_in_project(&grid), shape);
        e.translate(&grid, DVec3::new(-600.0, 0.0, 0.0));
        assert_eq!(e.tile, TileKey { x: 0, y: -1 }, "it moved to the tile that holds it now");
        let Shape::Point(p) = &e.shape else { unreachable!() };
        assert_eq!(p.at, DVec3::new(900.25, 979.5, 1.0));
    }

    #[test]
    fn definitions_round_trip_through_json_exactly() {
        let grid = TileGrid::default();
        let shape = Shape::Planar(PlanarDef {
            form: PlanarForm::Wall { start: DVec2::new(0.1, 0.2), end: DVec2::new(7.3, 0.2), base: 0.0, height: 2.7 },
            thickness: 0.214,
            openings: vec![Opening { along: 1.0, sill: 0.0, width: 0.93, height: 2.315, framing: None }],
            build_up: None,
        });
        let e = Element::in_project(&grid, "Wall 1", Some("level-path".into()), shape);
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Element>(&json).unwrap(), e, "{json}");
    }

    #[test]
    fn an_element_keeps_its_top_when_reshaped_and_reads_without_one() {
        let grid = TileGrid::default();
        let wall = |x: f64| Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: DVec2::new(x, 0.0), end: DVec2::new(x + 4.0, 0.0), base: 0.0, height: 3.0 }, thickness: 0.2, openings: Vec::new(), build_up: None });
        let mut e = Element::in_project(&grid, "Wall 1", Some("ground".into()), wall(0.0));
        assert_eq!(e.top, None);
        // a file of before tops reads without one
        let old = serde_json::to_string(&e).unwrap();
        assert!(!old.contains("top\""), "{old}");
        assert_eq!(serde_json::from_str::<Element>(&old).unwrap().top, None);
        e.id = ElementId(7);
        e.top = Some(Top { level: "first".into(), offset: -0.3 });
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Element>(&json).unwrap(), e, "{json}");
        // reshaped: the id, name, level and top stay, the shape is the new one
        let moved = e.reshaped(&grid, wall(5.0));
        assert_eq!((moved.id, moved.name.as_str(), moved.level.as_deref(), moved.top.clone()), (e.id, "Wall 1", Some("ground"), e.top.clone()));
        assert_eq!(moved.shape_in_project(&grid), wall(5.0));
    }

    #[test]
    fn an_element_keeps_its_grade_when_reshaped_and_reads_without_one() {
        let grid = TileGrid::default();
        let beam = |x: f64| Shape::Linear(LinearDef { start: DVec3::new(x, 0.0, 3.0), end: DVec3::new(x + 4.0, 0.0, 3.0), profile: ProfileDef::Rectangle { width: 0.2, height: 0.4 }, roll: 0.0 });
        let mut e = Element::in_project(&grid, "Beam 1", None, beam(0.0));
        let old = serde_json::to_string(&e).unwrap();
        assert!(!old.contains("grade"), "{old}");
        assert_eq!(serde_json::from_str::<Element>(&old).unwrap().grade, None);
        e.grade = Some("S355".into());
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Element>(&json).unwrap(), e, "{json}");
        assert_eq!(e.reshaped(&grid, beam(5.0)).grade.as_deref(), Some("S355"));
    }
}
