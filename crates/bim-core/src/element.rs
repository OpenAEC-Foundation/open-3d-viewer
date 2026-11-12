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
/// `rotation` radians about the vertical.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointDef {
    pub at: DVec3,
    #[serde(default)]
    pub rotation: f64,
    pub size: DVec3,
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
}

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
    },
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
}

/// A connection between two elements: two linear ones joined (butt, mitre), or any two with a
/// body where one cuts the other.
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
    pub shape: Shape,
}

impl Element {
    /// An element whose `shape` is given in project coordinates: it is placed in the tile that
    /// holds its placement origin, its coordinates turned into that tile's frame. Its id is set
    /// when it is added to a model.
    pub fn in_project(grid: &TileGrid, name: impl Into<String>, level: Option<String>, mut shape: Shape) -> Self {
        let tile = shape.placement().map_or(TileKey::default(), |p| grid.tile_of(p));
        shape.translate(-grid.origin(tile));
        Self { id: ElementId(0), name: name.into(), tile, level, shape }
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
            Shape::Point(_) => "IfcBuildingElementProxy",
            Shape::Linear(l) => linear_class(l.end - l.start),
            Shape::Planar(PlanarDef { form: PlanarForm::Wall { .. }, .. }) => "IfcWall",
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { .. }, .. }) => "IfcSlab",
            Shape::System(_) => "IfcCurtainWall",
            // a join with parts of its own is an assembly of them; one without is a relation only
            Shape::Connection(ConnectionDef { kind: ConnectionKind::EndPlate(_), .. }) => "IfcElementAssembly",
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
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes, top }, .. }) => {
                outline.iter_mut().chain(holes.iter_mut().flatten()).for_each(|p| *p += flat);
                *top += delta.z;
            }
            Shape::System(s) => {
                s.start += flat;
                s.end += flat;
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
        PlanarForm::Slab { outline, holes, top } => {
            let axes = Axes::new(DVec3::new(0.0, 0.0, top - thickness), DVec3::X, DVec3::Z)?;
            Ok((vec![Profile::new(outline.clone(), holes.clone())?], axes))
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
        Opening { along, sill, width, height }
    }

    /// The volume of a 5 × 3 m wall 0.2 thick with `area` square metres of its elevation taken out.
    fn less(area: f64) -> f64 {
        (5.0 * 3.0 - area) * 0.2
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
        let window = Opening { along: 2.0, sill: 0.9, width: 1.5, height: 1.2 };
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
        let s = planar_solid(&PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top: 3.0 }, thickness: 0.25, openings: Vec::new(), build_up: None }).unwrap();
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
        w.openings = vec![Opening { along: 1.0, sill: 0.9, width: 1.2, height: 1.2 }];
        let volume: f64 = planar_parts(&w).unwrap().iter().map(|(_, s)| s.volume()).sum();
        assert!((volume - (5.0 * 3.0 - 1.2 * 1.2) * 0.32).abs() < 1e-9, "{volume}");
    }

    #[test]
    fn a_layered_slab_has_its_first_layer_on_top() {
        let layer = |material: &str, thickness| LayerDef { material: material.into(), thickness, cavity: false };
        let outline = vec![DVec2::ZERO, DVec2::new(4.0, 0.0), DVec2::new(4.0, 3.0), DVec2::new(0.0, 3.0)];
        let slab = PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top: 3.0 }, thickness: 0.0, openings: Vec::new(), build_up: None }
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
        let s = SystemDef { start: DVec2::ZERO, end: DVec2::new(3.0, 0.0), base: 0.0, height: 3.0, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.03, system_type: None, openings: Vec::new() };
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
        let shape = Shape::Point(PointDef { at: DVec3::new(1500.25, -20.5, 1.0), rotation: 0.0, size: DVec3::ONE });
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
            openings: vec![Opening { along: 1.0, sill: 0.0, width: 0.93, height: 2.315 }],
            build_up: None,
        });
        let e = Element::in_project(&grid, "Wall 1", Some("level-path".into()), shape);
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Element>(&json).unwrap(), e, "{json}");
    }
}
