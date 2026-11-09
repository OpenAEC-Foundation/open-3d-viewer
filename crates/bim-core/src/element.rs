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

/// A planar element: a wall or a slab, `thickness` thick, with openings (walls only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanarDef {
    pub form: PlanarForm,
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openings: Vec<Opening>,
}

/// A system: a vertical domain on its line from `start` to `end`, from `base` up `height`,
/// divided into equal bays about `spacing` (along, up) apart. Members of `member` (width in the
/// face, depth across it) run on the division lines; panels `panel` thick fill the bays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemDef {
    pub start: DVec2,
    pub end: DVec2,
    pub base: f64,
    pub height: f64,
    pub spacing: DVec2,
    pub member: DVec2,
    pub panel: f64,
}

/// How a connection joins its members.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKind {
    /// The secondary member stops at the face of the primary one, which runs through.
    Butt,
    /// Both members are cut by the plane that halves the angle between them.
    Mitre,
    /// The primary element cuts its body out of the secondary one (a beam through a wall, a duct
    /// through a slab). Any two elements with a body; the secondary keeps what is left.
    Cut,
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
/// classes of their own).
#[derive(Debug, Clone, PartialEq)]
pub struct PartSolid {
    pub class: &'static str,
    pub solid: Solid,
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

/// The solid of a planar element.
pub fn planar_solid(p: &PlanarDef) -> Result<Solid, Refusal> {
    if !positive(p.thickness) {
        return Err(Refusal::NotPositive);
    }
    match &p.form {
        PlanarForm::Wall { start, end, base, height } => {
            let (axes, length) = wall_axes(*start, *end, *base, p.thickness)?;
            if !positive(*height) {
                return Err(Refusal::NotPositive);
            }
            Solid::extrude(&wall_elevation(length, *height, &p.openings)?, &axes, p.thickness)
        }
        PlanarForm::Slab { outline, holes, top } => {
            let axes = Axes::new(DVec3::new(0.0, 0.0, top - p.thickness), DVec3::X, DVec3::Z)?;
            Solid::extrude(&Profile::new(outline.clone(), holes.clone())?, &axes, p.thickness)
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

/// A wall's elevation, `length` × `height`, with its openings: those without a sill cut the bottom
/// edge, the others are holes. Openings must keep wall on both sides and above, and not touch.
fn wall_elevation(length: f64, height: f64, openings: &[Opening]) -> Result<Profile, Refusal> {
    let tol = LINEAR_TOLERANCE;
    for o in openings {
        if !positive(o.width) || !positive(o.height) || !o.along.is_finite() || !o.sill.is_finite() {
            return Err(Refusal::NotPositive);
        }
        if o.along <= tol || o.along + o.width >= length - tol || o.sill < -tol || o.sill + o.height >= height - tol {
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
    let door = |o: &&Opening| o.sill <= tol;
    let mut doors: Vec<&Opening> = openings.iter().filter(door).collect();
    doors.sort_by(|a, b| a.along.total_cmp(&b.along));
    let p = DVec2::new;
    let mut outer = vec![p(0.0, 0.0)];
    for d in doors {
        outer.extend([p(d.along, 0.0), p(d.along, d.height), p(d.along + d.width, d.height), p(d.along + d.width, 0.0)]);
    }
    outer.extend([p(length, 0.0), p(length, height), p(0.0, height)]);
    let holes = openings
        .iter()
        .filter(|o| !door(o))
        .map(|o| vec![p(o.along, o.sill), p(o.along, o.sill + o.height), p(o.along + o.width, o.sill + o.height), p(o.along + o.width, o.sill)])
        .collect();
    Profile::new(outer, holes)
}

/// The members and panels of a system.
pub fn system_parts(s: &SystemDef) -> Result<Vec<PartSolid>, Refusal> {
    let (w, depth) = (s.member.x, s.member.y);
    if ![s.height, s.spacing.x, s.spacing.y, w, depth, s.panel].into_iter().all(positive) {
        return Err(Refusal::NotPositive);
    }
    let d = s.end - s.start;
    let length = d.length();
    if !positive(length) {
        return Err(Refusal::ZeroLength);
    }
    let dir = (d / length).extend(0.0);
    let right = dir.cross(DVec3::Z);
    let bays = ((length / s.spacing.x).round() as usize).max(1);
    let rows = ((s.height / s.spacing.y).round() as usize).max(1);
    let (bay, row) = (length / bays as f64, s.height / rows as f64);
    if bay - w <= LINEAR_TOLERANCE || row - w <= LINEAR_TOLERANCE {
        return Err(Refusal::NotPositive);
    }
    let at = |u: f64, v: f64| s.start.extend(s.base) + dir * u + DVec3::Z * v;
    let mut parts = Vec::new();
    // mullions on every division along, the whole height
    let mullion = Profile::rectangle(w, depth)?;
    for i in 0..=bays {
        let axes = Axes::new(at(i as f64 * bay, 0.0), dir, DVec3::Z)?;
        parts.push(PartSolid { class: "IfcMember", solid: Solid::extrude(&mullion, &axes, s.height)? });
    }
    // transoms on every division up, between the mullions
    let transom = Profile::rectangle(depth, w)?;
    for i in 0..bays {
        for j in 0..=rows {
            let axes = Axes::new(at(i as f64 * bay + w / 2.0, j as f64 * row), right, dir)?;
            parts.push(PartSolid { class: "IfcMember", solid: Solid::extrude(&transom, &axes, bay - w)? });
        }
    }
    // a panel in every bay, centred in the face
    let p = DVec2::new;
    for i in 0..bays {
        for j in 0..rows {
            let (du, dv) = (bay - w, row - w);
            let corner = at(i as f64 * bay + w / 2.0, j as f64 * row + w / 2.0) - right * (s.panel / 2.0);
            let axes = Axes::new(corner, dir, right)?;
            let face = Profile::new(vec![p(0.0, 0.0), p(du, 0.0), p(du, dv), p(0.0, dv)], Vec::new())?;
            parts.push(PartSolid { class: "IfcPlate", solid: Solid::extrude(&face, &axes, s.panel)? });
        }
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(openings: Vec<Opening>) -> PlanarDef {
        PlanarDef { form: PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(5.0, 0.0), base: 0.0, height: 3.0 }, thickness: 0.2, openings }
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
        let door = Opening { along: 0.5, sill: 0.0, width: 0.9, height: 2.1 };
        let window = Opening { along: 2.0, sill: 0.9, width: 1.5, height: 1.2 };
        let s = planar_solid(&wall(vec![door, window])).unwrap();
        let expected = (5.0 * 3.0 - 0.9 * 2.1 - 1.5 * 1.2) * 0.2;
        assert!((s.volume() - expected).abs() < 1e-12, "{} vs {expected}", s.volume());
    }

    #[test]
    fn openings_that_do_not_fit_are_refused() {
        let at = |along, sill, width, height| Opening { along, sill, width, height };
        assert_eq!(planar_solid(&wall(vec![at(0.0, 0.9, 1.0, 1.0)])), Err(Refusal::OpeningOutside), "at the start");
        assert_eq!(planar_solid(&wall(vec![at(4.5, 0.9, 0.5, 1.0)])), Err(Refusal::OpeningOutside), "at the end");
        assert_eq!(planar_solid(&wall(vec![at(1.0, 2.0, 1.0, 1.0)])), Err(Refusal::OpeningOutside), "through the top");
        assert_eq!(planar_solid(&wall(vec![at(1.0, -0.1, 1.0, 1.0)])), Err(Refusal::OpeningOutside), "below the base");
        assert_eq!(planar_solid(&wall(vec![at(1.0, 0.9, 1.0, 1.0), at(1.5, 1.5, 1.0, 1.0)])), Err(Refusal::OpeningsOverlap));
        assert_eq!(planar_solid(&wall(vec![at(1.0, 0.9, 1.0, 1.0), at(2.0, 0.9, 1.0, 1.0)])), Err(Refusal::OpeningsOverlap), "touching");
        assert_eq!(planar_solid(&wall(vec![at(1.0, 0.9, 0.0, 1.0)])), Err(Refusal::NotPositive));
    }

    #[test]
    fn a_slab_hangs_under_its_top() {
        let outline = vec![DVec2::ZERO, DVec2::new(4.0, 0.0), DVec2::new(4.0, 3.0), DVec2::new(0.0, 3.0)];
        let s = planar_solid(&PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top: 3.0 }, thickness: 0.25, openings: Vec::new() }).unwrap();
        let (lo, hi) = s.bounds();
        assert_eq!((lo.z, hi.z), (2.75, 3.0));
        assert!((s.volume() - 4.0 * 3.0 * 0.25).abs() < 1e-12);
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
        let s = SystemDef { start: DVec2::ZERO, end: DVec2::new(3.0, 0.0), base: 0.0, height: 3.0, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.03 };
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
        });
        let e = Element::in_project(&grid, "Wall 1", Some("level-path".into()), shape);
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Element>(&json).unwrap(), e, "{json}");
    }
}
