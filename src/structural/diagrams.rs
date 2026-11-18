//! What a structural analysis model shows of its loads and results in the 3D view, as lines
//! over the model and a few figures beside them.
//!
//! - **Internal forces**: one of the six (N, Vy, Vz, Mx, My, Mz) of a result set, drawn along
//!   every member as a line off its axis with a stroke at each station. N, Vz, Mx and My are set
//!   out along the member's local z, Vy and Mz along its local y; a bending moment lies on the
//!   side of the fibre it stretches (+My stretches +z, +Mz stretches −y). The largest value of
//!   the model takes [`REACH`] of the model's size.
//! - **Deformed shape**: the members between their nodes as displaced, the largest displacement
//!   taking [`REACH`] of the model's size; a member with a section also as the solid of its
//!   profile there. The file gives displacements at the nodes only, so a member stays straight
//!   between them.
//! - **Reactions**: an arrow at every supported node, the force the support puts on the structure.
//! - **Loads** of a load case: arrows at nodes, along members and on surfaces.
//! - **Supports and releases**: at every supported node a symbol of what it holds (a square for a
//!   fixed support, a triangle for a pinned one, a triangle on a line for a roller, a diamond for
//!   the rest) and near every released member end a small circle.
//!
//! All of it is derived from the document; nothing is computed that the file does not hold.

use super::StructuralPart;
use crate::model::Origin;
use crate::views3d::Colored;
use bim_structural::*;
use glam::DVec3;

/// How far the largest value of a diagram reaches, as a part of the model's size.
pub const REACH: f64 = 0.12;

const POSITIVE: [u8; 4] = [37, 99, 235, 255];
const NEGATIVE: [u8; 4] = [220, 38, 38, 255];
const DEFORMED: [u8; 4] = [22, 163, 74, 255];
const DEFORMED_SOLID: [u8; 4] = [74, 222, 128, 255];
const REACTION: [u8; 4] = [124, 58, 237, 255];
const LOAD: [u8; 4] = [217, 119, 6, 255];
const SUPPORT: [u8; 4] = [38, 166, 154, 255];
const SUPPORT_EDGE: [u8; 4] = [17, 78, 74, 255];
/// The reactions along x, y and z, in the colours of the axes.
const ALONG: [[u8; 4]; 3] = [[220, 38, 38, 255], [22, 163, 74, 255], [37, 99, 235, 255]];
const RELEASE: [u8; 4] = [219, 39, 119, 255];
/// The size of a support symbol, as a part of the model's size; it does not follow the scale.
const SYMBOL: f64 = 0.02;

/// An internal force of a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Force {
    N,
    Vy,
    Vz,
    Mx,
    My,
    Mz,
}

impl Force {
    pub const ALL: [Force; 6] = [Force::N, Force::Vy, Force::Vz, Force::Mx, Force::My, Force::Mz];

    /// Its place among the six components of a result table.
    pub fn slot(self) -> usize {
        self as usize
    }

    pub fn symbol(self) -> &'static str {
        ["N", "Vy", "Vz", "Mx", "My", "Mz"][self.slot()]
    }

    /// The unit its values are written in: kilonewtons, or kilonewton metres for a moment.
    pub fn unit(self) -> &'static str {
        if self.slot() < 3 { "kN" } else { "kNm" }
    }

    pub fn parse(s: &str) -> Option<Force> {
        Force::ALL.into_iter().find(|f| f.symbol().eq_ignore_ascii_case(s.trim()))
    }
}

/// How the line of an internal force is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    /// The line with a stroke to the member at every station.
    #[default]
    Strokes,
    /// The line alone, closed onto the member at its ends.
    Outline,
}

/// Where the value of an internal force is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Values {
    /// At the largest of the model.
    #[default]
    Model,
    /// At the largest of every member.
    Members,
}

/// A filled triangle in display coordinates with its colour, as the cut faces are.
pub type Face = ([[f32; 3]; 3], [[u8; 4]; 3]);

/// How the deformed shape is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeformedAs {
    /// The members' lines, and the solid of every member that has a section.
    #[default]
    Profiles,
    Lines,
}

/// How the reactions are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReactionsAs {
    /// One arrow per node: the whole force.
    #[default]
    Resultant,
    /// An arrow along x, along y and along z, each with its value.
    Components,
}

/// The kinds of things whose size is set apart from the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Forces,
    Deformed,
    Loads,
    Reactions,
    Symbols,
}

impl Part {
    pub const ALL: [Part; 5] = [Part::Forces, Part::Deformed, Part::Loads, Part::Reactions, Part::Symbols];
}

/// What of the loads and results is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Shown {
    /// The result set, by its place among the model's result sets.
    pub set: usize,
    pub force: Option<Force>,
    pub deformed: bool,
    pub reactions: bool,
    /// The symbols of the supports and of the released member ends.
    pub supports: bool,
    /// The load case whose loads are drawn, by its place among the model's load cases.
    pub loads: Option<usize>,
    /// Larger or smaller than [`REACH`]: 1 as it is.
    pub scale: f32,
    pub style: Style,
    pub values: Values,
    pub deformed_as: DeformedAs,
    pub reactions_as: ReactionsAs,
    /// The plates as volumes in the colours of a value over their mesh: the displacement (0),
    /// or the field of that place, counted from 1. With the lines of the mesh, or without.
    pub plates: bool,
    pub plate_value: u8,
    pub plate_mesh: bool,
    /// How large each kind of thing is drawn, times the scale: 1 (or 0, unset) as it is.
    pub sizes: [f32; 5],
    /// The part of a value's range the colours run over, from 0 (its lowest) to 1 (its highest);
    /// `colour_to` of 0 is unset and stands for 1. In `bands` steps, or (0) without steps.
    pub colour_from: f32,
    pub colour_to: f32,
    pub bands: u8,
    /// The node glyphs, the member axes and the figures beside the lines, left out.
    pub hide_nodes: bool,
    pub hide_members: bool,
    pub hide_values: bool,
}

/// A change of what is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Change {
    /// Draw this internal force, or none when it is drawn already.
    Force(Force),
    Deformed,
    Reactions,
    Supports,
    /// The loads of the load case shown, on or off.
    Loads,
    /// The next (true) or previous load case, and result set.
    Case(bool),
    Set(bool),
    /// Larger (true) or smaller.
    Scale(bool),
    /// The result set, or the load case whose loads are drawn (none: no loads), by its place.
    SetAt(usize),
    CaseAt(Option<usize>),
    ScaleTo(f32),
    Style(Style),
    Values(Values),
    DeformedAs(DeformedAs),
    ReactionsAs(ReactionsAs),
    /// The plates in colours, on or off (switched, or given); the value they show; their mesh.
    Plates,
    PlatesOn(bool),
    PlateValue(u8),
    PlateMeshOn(bool),
    /// The size of a kind of thing; the part of the range the colours run over; their steps.
    Size(Part, f32),
    ColourFrom(f32),
    ColourTo(f32),
    Bands(u8),
    /// Each given as it is to be, not switched: the internal force (none: no line), the deformed
    /// shape, the reactions, the supports, and whether nodes, members and figures show.
    ForceIs(Option<Force>),
    DeformedOn(bool),
    ReactionsOn(bool),
    SupportsOn(bool),
    NodesOn(bool),
    MembersOn(bool),
    ValuesOn(bool),
}

impl Shown {
    /// Takes a change; `cases` and `sets` are how many load cases and result sets the model has.
    pub fn apply(&mut self, change: Change, cases: usize, sets: usize) {
        let step = |at: usize, n: usize, on: bool| if n == 0 { 0 } else if on { (at + 1) % n } else { (at + n - 1) % n };
        match change {
            Change::Force(force) => self.force = (self.force != Some(force)).then_some(force),
            Change::Deformed => self.deformed = !self.deformed,
            Change::Reactions => self.reactions = !self.reactions,
            Change::Supports => self.supports = !self.supports,
            Change::Loads => self.loads = if self.loads.is_some() || cases == 0 { None } else { Some(0) },
            Change::Case(next) => self.loads = (cases > 0).then(|| step(self.loads.unwrap_or(if next { cases - 1 } else { 0 }), cases, next)),
            Change::Set(next) => self.set = step(self.set.min(sets.saturating_sub(1)), sets, next),
            Change::SetAt(at) => self.set = at.min(sets.saturating_sub(1)),
            Change::CaseAt(at) => self.loads = at.filter(|k| *k < cases),
            Change::ScaleTo(scale) => self.scale = if scale.is_finite() { scale.clamp(0.05, 20.0) } else { self.scale },
            Change::DeformedAs(how) => self.deformed_as = how,
            Change::ReactionsAs(how) => self.reactions_as = how,
            Change::Plates => self.plates = !self.plates,
            Change::PlatesOn(on) => self.plates = on,
            Change::PlateValue(value) => self.plate_value = value,
            Change::PlateMeshOn(on) => self.plate_mesh = on,
            Change::Size(part, size) if size.is_finite() => self.sizes[part as usize] = size.clamp(0.05, 20.0),
            Change::Size(..) => {}
            // the colours keep a twentieth of the range between where they start and end
            Change::ColourFrom(at) if at.is_finite() => self.colour_from = at.clamp(0.0, self.colours().1 - 0.05),
            Change::ColourTo(at) if at.is_finite() => self.colour_to = at.clamp(self.colour_from + 0.05, 1.0),
            Change::ColourFrom(_) | Change::ColourTo(_) => {}
            Change::Bands(bands) => self.bands = bands.min(24),
            Change::ForceIs(force) => self.force = force,
            Change::DeformedOn(on) => self.deformed = on,
            Change::ReactionsOn(on) => self.reactions = on,
            Change::SupportsOn(on) => self.supports = on,
            Change::NodesOn(on) => self.hide_nodes = !on,
            Change::MembersOn(on) => self.hide_members = !on,
            Change::ValuesOn(on) => self.hide_values = !on,
            Change::Style(style) => self.style = style,
            Change::Values(values) => self.values = values,
            Change::Scale(larger) => self.scale = (if self.scale > 0.0 { self.scale } else { 1.0 } * if larger { 1.5 } else { 1.0 / 1.5 }).clamp(0.05, 20.0),
        }
    }

    /// How large `part` is drawn, beside the scale.
    pub fn size(&self, part: Part) -> f64 {
        let size = self.sizes[part as usize];
        if size > 0.0 { f64::from(size) } else { 1.0 }
    }

    /// The part of a value's range the colours run over.
    pub fn colours(&self) -> (f32, f32) {
        (self.colour_from, if self.colour_to > 0.0 { self.colour_to } else { 1.0 })
    }

    /// Where on the colour scale a value at `t` of its range (0 lowest, 1 highest) lies.
    pub fn on_scale(&self, t: f64) -> f64 {
        let (from, to) = self.colours();
        let t = ((t - f64::from(from)) / f64::from(to - from).max(1e-6)).clamp(0.0, 1.0);
        match f64::from(self.bands) {
            bands if bands > 0.0 => ((t * bands).floor().min(bands - 1.0) + 0.5) / bands,
            _ => t,
        }
    }

    /// What is shown, as a view keeps it.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "set": self.set, "force": self.force.map(|f| f.symbol()), "deformed": self.deformed, "reactions": self.reactions, "supports": self.supports,
            "loads": self.loads, "scale": self.scale, "outline": self.style == Style::Outline, "values_per_member": self.values == Values::Members,
            "deformed_lines": self.deformed_as == DeformedAs::Lines, "reactions_xyz": self.reactions_as == ReactionsAs::Components,
            "plates": self.plates, "plate_value": self.plate_value, "plate_mesh": self.plate_mesh, "sizes": self.sizes,
            "colour_from": self.colour_from, "colour_to": self.colour_to, "bands": self.bands,
            "hide_nodes": self.hide_nodes, "hide_members": self.hide_members, "hide_values": self.hide_values,
        })
    }

    /// What a view kept; what it does not say is as when nothing is shown.
    pub fn from_json(v: &serde_json::Value) -> Option<Shown> {
        let v = v.as_object()?;
        let on = |k: &str| v.get(k).and_then(serde_json::Value::as_bool).unwrap_or(false);
        let number = |k: &str| v.get(k).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        let mut sizes = [0.0; 5];
        for (size, given) in sizes.iter_mut().zip(v.get("sizes").and_then(serde_json::Value::as_array).into_iter().flatten()) {
            *size = given.as_f64().unwrap_or(0.0) as f32;
        }
        Some(Shown {
            set: number("set") as usize,
            force: v.get("force").and_then(serde_json::Value::as_str).and_then(|s| Force::ALL.into_iter().find(|f| f.symbol() == s)),
            deformed: on("deformed"),
            reactions: on("reactions"),
            supports: on("supports"),
            loads: v.get("loads").and_then(serde_json::Value::as_u64).map(|k| k as usize),
            scale: number("scale") as f32,
            style: if on("outline") { Style::Outline } else { Style::Strokes },
            values: if on("values_per_member") { Values::Members } else { Values::Model },
            deformed_as: if on("deformed_lines") { DeformedAs::Lines } else { DeformedAs::Profiles },
            reactions_as: if on("reactions_xyz") { ReactionsAs::Components } else { ReactionsAs::Resultant },
            plates: on("plates"),
            plate_value: number("plate_value") as u8,
            plate_mesh: on("plate_mesh"),
            sizes,
            colour_from: number("colour_from") as f32,
            colour_to: number("colour_to") as f32,
            bands: number("bands") as u8,
            hide_nodes: on("hide_nodes"),
            hide_members: on("hide_members"),
            hide_values: on("hide_values"),
        })
    }

    pub fn anything(&self) -> bool {
        self.force.is_some() || self.deformed || self.reactions || self.supports || self.plates || self.loads.is_some()
    }
}

/// The lines and figures of `shown` for `parts`, in display coordinates.
pub fn draw(parts: &[StructuralPart], origins: &[Origin], shown: &Shown) -> (Vec<Colored>, Vec<(String, [f32; 3])>) {
    let drawn = draw_all(parts, origins, shown);
    (drawn.lines, drawn.labels)
}

/// What [`draw_all`] gives: lines, filled triangles (arrow heads, supports, the solids of the
/// deformed shape) and figures.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Drawing {
    pub lines: Vec<Colored>,
    pub faces: Vec<Face>,
    pub labels: Vec<(String, [f32; 3])>,
}

/// The lines, filled triangles and figures of `shown` for `parts`, in display coordinates.
pub fn draw_all(parts: &[StructuralPart], origins: &[Origin], shown: &Shown) -> Drawing {
    let (mut lines, mut faces, mut labels) = (Vec::new(), Vec::new(), Vec::new());
    if !shown.anything() {
        return Drawing::default();
    }
    for part in parts {
        let doc = &part.doc;
        let origin = origins.get(part.model as usize).copied().unwrap_or_default();
        let at = |p: DVec3| origin.display(p).as_vec3().to_array();
        // the model's size: the diagonal of its nodes
        let (low, high) = doc.nodes.iter().fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(lo, hi), n| (lo.min(n.p), hi.max(n.p)));
        let size = if low.cmple(high).all() { (high - low).length().max(1.0) } else { 1.0 };
        let reach = size * REACH * f64::from(if shown.scale > 0.0 { shown.scale } else { 1.0 });
        let mut pen = Pen { lines: &mut lines, faces: &mut faces, at: &at };
        let results = doc.results.get(shown.set).and_then(|r| r.inline.as_ref());
        if let (Some(force), Some(table)) = (shown.force, results.and_then(|r| r.member_forces.as_ref())) {
            for (value, p) in forces(doc, table, force, reach * shown.size(Part::Forces), shown, &mut pen) {
                labels.push((format!("{} {}", force.symbol(), figure(value / 1e3, force.unit())), at(p)));
            }
        }
        // one scale for the nodes of the model and those of the plates' mesh
        let mesh = results.and_then(|r| r.surface_mesh.as_ref());
        let most = mesh.map_or(0.0, |m| m.displacements.iter().filter(|u| u.is_finite()).fold(0.0_f64, |a, u| a.max(u.length())));
        let most = results.and_then(|r| r.node_displacements.as_ref()).map_or(most, |t| most.max(largest_displacement(t)));
        if let (true, Some(mesh)) = (shown.plates, mesh) {
            let moved = if shown.deformed && most > 0.0 { reach * shown.size(Part::Deformed) / most } else { 0.0 };
            if let Some((text, p)) = plates(mesh, shown, moved, &mut pen) {
                labels.push((text, at(p)));
            }
        }
        if let (true, Some(table)) = (shown.deformed, results.and_then(|r| r.node_displacements.as_ref())) {
            if let Some((value, p)) = deformed(doc, table, reach * shown.size(Part::Deformed), most, shown.deformed_as, &mut pen) {
                labels.push((format!("u {}", figure(value * 1e3, "mm")), at(p)));
            }
        }
        if let (true, Some(table)) = (shown.reactions, results.and_then(|r| r.reactions.as_ref())) {
            for (text, p) in reactions(doc, table, reach * shown.size(Part::Reactions), shown.reactions_as, &mut pen) {
                labels.push((text, at(p)));
            }
        }
        if let Some(case) = shown.loads.and_then(|k| doc.load_cases.get(k)) {
            loads(doc, case.id, reach * 0.6 * shown.size(Part::Loads), &mut pen);
        }
        if shown.supports {
            supports(doc, size * SYMBOL * shown.size(Part::Symbols), &mut pen);
        }
    }
    if shown.hide_values {
        labels.clear();
    }
    Drawing { lines, faces, labels }
}

/// A figure with its unit, in three significant digits.
fn figure(value: f64, unit: &str) -> String {
    let digits = if value.abs() >= 100.0 { 0 } else if value.abs() >= 10.0 { 1 } else { 2 };
    format!("{value:.digits$} {unit}")
}

/// Draws lines and filled triangles in display coordinates from world points.
struct Pen<'a> {
    lines: &'a mut Vec<Colored>,
    faces: &'a mut Vec<Face>,
    at: &'a dyn Fn(DVec3) -> [f32; 3],
}

/// Where the light that shades the filled triangles comes from.
const LIGHT: DVec3 = DVec3::new(0.35, 0.25, 0.9);

impl Pen<'_> {
    fn line(&mut self, a: DVec3, b: DVec3, color: [u8; 4]) {
        if a.is_finite() && b.is_finite() && a != b {
            self.lines.push(([(self.at)(a), (self.at)(b)], color));
        }
    }

    /// A filled triangle, darker the more its face turns from the light.
    fn face(&mut self, corners: [DVec3; 3], normal: DVec3, color: [u8; 4]) {
        if !corners.iter().all(|c| c.is_finite()) {
            return;
        }
        let lit = 0.5 + 0.5 * normal.normalize_or_zero().dot(LIGHT.normalize()).abs();
        let shade = |c: u8| (f64::from(c) * lit).round().clamp(0.0, 255.0) as u8;
        self.faces.push((corners.map(|c| (self.at)(c)), [[shade(color[0]), shade(color[1]), shade(color[2]), color[3]]; 3]));
    }

    /// A flat face of four corners in order.
    fn quad(&mut self, c: [DVec3; 4], color: [u8; 4]) {
        let normal = (c[1] - c[0]).cross(c[2] - c[0]);
        self.face([c[0], c[1], c[2]], normal, color);
        self.face([c[0], c[2], c[3]], normal, color);
    }

    /// A block: the square of half-width `half` around `top` in the plane of `u` and `v`, and
    /// `depth` down along `w` from there.
    fn block(&mut self, top: DVec3, u: DVec3, v: DVec3, w: DVec3, half: f64, depth: f64, color: [u8; 4]) {
        let corner = |a: f64, b: f64, down: f64| top + u * (a * half) + v * (b * half) - w * down;
        let ring = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
        self.quad(ring.map(|(a, b)| corner(a, b, 0.0)), color);
        self.quad(ring.map(|(a, b)| corner(a, b, depth)), color);
        for k in 0..4 {
            let ((a, b), (c, d)) = (ring[k], ring[(k + 1) % 4]);
            self.quad([corner(a, b, 0.0), corner(c, d, 0.0), corner(c, d, depth), corner(a, b, depth)], color);
        }
    }

    /// A round bar or cone from `a` (radius `ra`) to `b` (radius `rb`), closed at both ends.
    fn round(&mut self, a: DVec3, b: DVec3, ra: f64, rb: f64, color: [u8; 4]) {
        let along = (b - a).normalize_or_zero();
        if along == DVec3::ZERO {
            return;
        }
        let side = frame::default_z_ref(along).cross(along).normalize_or_zero();
        let up = along.cross(side);
        const SIDES: usize = 20;
        let around = |k: usize| {
            let t = k as f64 / SIDES as f64 * std::f64::consts::TAU;
            side * t.cos() + up * t.sin()
        };
        for k in 0..SIDES {
            let (d0, d1) = (around(k), around(k + 1));
            let (a0, a1, b0, b1) = (a + d0 * ra, a + d1 * ra, b + d0 * rb, b + d1 * rb);
            let normal = (a1 - a0).cross(b0 - a0);
            self.face([a0, a1, b1], normal, color);
            self.face([a0, b1, b0], normal, color);
            self.face([a, a1, a0], along, color);
            self.face([b, b0, b1], along, color);
        }
    }

    /// An arrow from `tail` to `tip`: a round shaft and a cone for its head, a quarter of its
    /// length. Its centre line is drawn too, for where only lines show.
    fn arrow(&mut self, tail: DVec3, tip: DVec3, color: [u8; 4]) {
        let shaft = tip - tail;
        let length = shaft.length();
        if length <= 1e-9 || !length.is_finite() {
            return;
        }
        self.line(tail, tip, color);
        let neck = tip - shaft * 0.25;
        self.round(tail, neck, length * 0.018, length * 0.018, color);
        self.round(neck, tip, length * 0.07, 0.0, color);
    }
}

/// The frame of a straight member.
fn frame_of(doc: &AnalysisModel, member: &Member) -> Option<frame::MemberFrame> {
    let (a, b) = doc.member_ends(member)?;
    frame::member_frame(a, b, member.z_ref).or_else(|_| frame::member_frame(a, b, frame::default_z_ref(b - a))).ok()
}

/// The line of one internal force along every member; the largest value of the model, or of
/// every member, and where it stands.
fn forces(doc: &AnalysisModel, table: &results::MemberTable, force: Force, reach: f64, shown: &Shown, pen: &mut Pen) -> Vec<(f64, DVec3)> {
    let Some(values) = table.comps.get(force.slot()) else { return Vec::new() };
    let most = values.iter().copied().filter(|v| v.is_finite()).fold(0.0_f64, |m, v| m.max(v.abs()));
    if most <= 0.0 {
        return Vec::new();
    }
    let mut peaks: Vec<(f64, DVec3)> = Vec::new();
    let mut peak: Option<(f64, DVec3)> = None;
    for (k, id) in table.member.iter().enumerate() {
        let (Some(member), Some(&from), Some(&to)) = (doc.member(*id), table.offsets.get(k), table.offsets.get(k + 1)) else { continue };
        let Some(f) = frame_of(doc, member) else { continue };
        // set out along local z, or along local y; a moment on the side it stretches
        let out = match force {
            Force::N | Force::Vz | Force::Mx | Force::My => f.z,
            Force::Vy => f.y,
            Force::Mz => -f.y,
        };
        let stations: Vec<(f64, f64)> = (from as usize..to as usize).filter_map(|s| Some((*table.x.get(s)?, *values.get(s)?))).filter(|(x, v)| x.is_finite() && v.is_finite()).collect();
        let mut before: Option<(DVec3, DVec3, f64)> = None;
        if shown.values == Values::Members {
            peak = None;
        }
        for (at, &(x, v)) in stations.iter().enumerate() {
            let foot = f.origin + f.x * x;
            let head = foot + out * (v / most * reach);
            let color = if v >= 0.0 { POSITIVE } else { NEGATIVE };
            if shown.style == Style::Strokes || at == 0 || at + 1 == stations.len() {
                pen.line(foot, head, color);
            }
            if let Some((_, last, was)) = before {
                pen.line(last, head, if (was + v) >= 0.0 { POSITIVE } else { NEGATIVE });
            }
            if peak.is_none_or(|(p, _)| v.abs() > p.abs()) {
                peak = Some((v, head));
            }
            before = Some((foot, head, v));
        }
        if shown.values == Values::Members {
            peaks.extend(peak.filter(|(v, _)| v.abs() > most * 1e-6));
        }
    }
    if shown.values == Values::Model {
        peaks.extend(peak);
    }
    peaks
}

/// The members between their displaced nodes, as lines and (`how`) as the solids of their
/// profiles; the largest displacement and where it is.
fn deformed(doc: &AnalysisModel, table: &results::NodeTable, reach: f64, most: f64, how: DeformedAs, pen: &mut Pen) -> Option<(f64, DVec3)> {
    let moved: std::collections::HashMap<Id, DVec3> = table.node.iter().enumerate().filter_map(|(k, id)| Some((*id, DVec3::new(*table.comps[0].get(k)?, *table.comps[1].get(k)?, *table.comps[2].get(k)?)))).filter(|(_, u)| u.is_finite()).collect();
    let most = moved.values().fold(most, |m, u| m.max(u.length()));
    if most <= 0.0 {
        return None;
    }
    let place = |id: Id| doc.node(id).map(|n| n.p + moved.get(&id).copied().unwrap_or(DVec3::ZERO) * (reach / most));
    // a profile's outline is made once, however many members have it
    let mut outlines: std::collections::HashMap<Id, Option<Vec<section::Loop>>> = std::collections::HashMap::new();
    for member in &doc.members {
        let (Some(a), Some(b)) = (member.nodes.first().and_then(|id| place(*id)), member.nodes.last().and_then(|id| place(*id))) else { continue };
        pen.line(a, b, DEFORMED);
        if how != DeformedAs::Profiles {
            continue;
        }
        let Some(section) = member.section.and_then(|(id, _)| doc.section(id)) else { continue };
        let loops = outlines.entry(section.id).or_insert_with(|| section::outline(&section.shape, 0.001, 8).ok().map(|o| o.loops()));
        let (Some(loops), Ok(f)) = (loops.as_ref(), frame::member_frame(a, b, member.z_ref).or_else(|_| frame::member_frame(a, b, frame::default_z_ref(b - a)))) else { continue };
        for (corners, normal) in super::solid::sweep(loops, &f) {
            pen.face(corners, normal, DEFORMED_SOLID);
        }
    }
    let (id, u) = moved.iter().max_by(|a, b| a.1.length().total_cmp(&b.1.length()))?;
    Some((u.length(), place(*id)?))
}

/// The largest displacement of a node in `table`.
fn largest_displacement(table: &results::NodeTable) -> f64 {
    (0..table.node.len()).filter_map(|k| Some(DVec3::new(*table.comps[0].get(k)?, *table.comps[1].get(k)?, *table.comps[2].get(k)?))).filter(|u| u.is_finite()).fold(0.0_f64, |m, u| m.max(u.length()))
}

/// The colours of a value, from its lowest (blue) through green and yellow to its highest (red).
const SCALE: [[f64; 3]; 5] = [[43.0, 131.0, 186.0], [171.0, 221.0, 164.0], [255.0, 255.0, 191.0], [253.0, 174.0, 97.0], [215.0, 25.0, 28.0]];
const MESH_LINE: [u8; 4] = [30, 41, 59, 255];

/// The colour of `t`, from 0 (lowest) to 1 (highest).
pub fn scale_colour(t: f64) -> [u8; 3] {
    let at = (if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 }) * (SCALE.len() - 1) as f64;
    let k = (at.floor() as usize).min(SCALE.len() - 2);
    let f = at - k as f64;
    [0, 1, 2].map(|c| (SCALE[k][c] + (SCALE[k + 1][c] - SCALE[k][c]) * f).round() as u8)
}

/// A value over the plates' mesh as it is shown: its name, the unit and factor it is written
/// in, and the value at every mesh node (SI).
pub struct PlateValue {
    pub name: String,
    pub unit: &'static str,
    pub factor: f64,
    pub values: Vec<f64>,
}

impl PlateValue {
    /// The lowest and highest value, in SI.
    pub fn range(&self) -> (f64, f64) {
        let (low, high) = self.values.iter().filter(|v| v.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        if low <= high { (low, high) } else { (0.0, 0.0) }
    }

    /// A value as it is written: in the shown unit, three significant digits.
    pub fn written(&self, value: f64) -> String {
        figure(value * self.factor, self.unit)
    }
}

/// The stresses that follow from the moments (`mx`, `my`, `mxy`, per metre, positive with tension
/// at the bottom: the side against the element's normal) and the membrane forces (`nx`, `ny`,
/// `nxy`, per metre, tension positive) of a mesh, at the top and at the bottom of the plate:
/// along x and y, the shear, the two principal stresses and the equivalent (von Mises) stress.
const STRESSES: [&str; 6] = ["σx", "σy", "τxy", "σ1", "σ2", "σvm"];
const SIDES: [(&str, f64); 2] = [("top", -1.0), ("bottom", 1.0)];

fn field<'a>(mesh: &'a results::SurfaceTable, name: &str) -> Option<&'a [f64]> {
    mesh.fields.iter().find(|f| f.name == name).map(|f| f.values.as_slice())
}

/// Whether the mesh holds something stresses are made of; what it lacks counts as nothing.
fn has_stresses(mesh: &results::SurfaceTable) -> bool {
    ["mx", "my", "mxy", "nx", "ny", "nxy"].iter().any(|n| field(mesh, n).is_some())
}

/// The names of the values a mesh can be coloured by: the displacement, its fields, and the
/// stresses at the top and the bottom that follow from them.
pub fn plate_values(mesh: &results::SurfaceTable) -> Vec<String> {
    let stresses = SIDES.iter().flat_map(|(side, _)| STRESSES.iter().map(move |s| format!("{s} {side}"))).filter(|_| has_stresses(mesh));
    std::iter::once("u".to_string()).chain(mesh.fields.iter().map(|f| f.name.clone())).chain(stresses).collect()
}

/// Stress `which` (its place in [`STRESSES`]) at the side of sign `side`, at every mesh node.
fn stress(mesh: &results::SurfaceTable, which: usize, side: f64) -> Vec<f64> {
    let n = mesh.points.len();
    let mut thick = vec![0.0_f64; n];
    for k in 0..mesh.elements.len() {
        for c in mesh.corners(k) {
            if let Some(t) = thick.get_mut(*c as usize) {
                *t = t.max(mesh.thickness.get(k).copied().unwrap_or(0.0));
            }
        }
    }
    let of = |name: &str, node: usize| field(mesh, name).and_then(|v| v.get(node)).copied().unwrap_or(0.0);
    (0..n)
        .map(|node| {
            let t = thick[node];
            if !(t > 0.0) {
                return f64::NAN;
            }
            let at = |force: &str, moment: &str| of(force, node) / t + side * 6.0 * of(moment, node) / (t * t);
            let (x, y, shear) = (at("nx", "mx"), at("ny", "my"), at("nxy", "mxy"));
            let (mean, radius) = ((x + y) / 2.0, (((x - y) / 2.0).powi(2) + shear * shear).sqrt());
            match which {
                0 => x,
                1 => y,
                2 => shear,
                3 => mean + radius,
                4 => mean - radius,
                _ => (x * x - x * y + y * y + 3.0 * shear * shear).sqrt(),
            }
        })
        .collect()
}

/// The value `which` of the mesh: the size of the displacement (0), or a field (from 1; one that
/// is not there gives the displacement).
pub fn plate_value(mesh: &results::SurfaceTable, which: u8) -> PlateValue {
    // after the fields: the stresses, the six at the top and then the six at the bottom
    let derived = usize::from(which).checked_sub(1 + mesh.fields.len()).filter(|k| *k < STRESSES.len() * SIDES.len() && has_stresses(mesh));
    if let Some(k) = derived {
        let (side, sign) = SIDES[k / STRESSES.len()];
        let which = k % STRESSES.len();
        return PlateValue { name: format!("{} {side}", STRESSES[which]), unit: "MPa", factor: 1e-6, values: stress(mesh, which, sign) };
    }
    match which.checked_sub(1).and_then(|k| mesh.fields.get(k as usize)) {
        None => PlateValue { name: "u".into(), unit: "mm", factor: 1e3, values: mesh.displacements.iter().map(|u| u.length()).collect() },
        Some(field) => {
            // forces and moments in kilo, stresses in mega, lengths in milli; else as it is
            let (unit, factor) = match field.unit.as_str() {
                "N·m/m" => ("kNm/m", 1e-3),
                "N/m" => ("kN/m", 1e-3),
                "N" => ("kN", 1e-3),
                "Pa" => ("MPa", 1e-6),
                "m" => ("mm", 1e3),
                _ => ("", 1.0),
            };
            PlateValue { name: field.name.clone(), unit, factor, values: field.values.clone() }
        }
    }
}

/// The plates as volumes: every element of the mesh with its thickness, moved by its
/// displacements times `moved`, in the colours of the value shown; the edge of the plates closed,
/// and the lines of the mesh on top when asked. Gives the value furthest from zero and where.
fn plates(mesh: &results::SurfaceTable, shown: &Shown, moved: f64, pen: &mut Pen) -> Option<(String, DVec3)> {
    let n = mesh.points.len();
    if n == 0 || mesh.displacements.len() != n {
        return None;
    }
    let value = plate_value(mesh, shown.plate_value);
    let (low, high) = value.range();
    let span = (high - low).max(f64::MIN_POSITIVE);
    let mid: Vec<DVec3> = (0..n).map(|k| mesh.points[k] + mesh.displacements[k] * moved).collect();
    // at every node: the normal of the elements around it, and the thickest of them
    let (mut normal, mut thick) = (vec![DVec3::ZERO; n], vec![0.0_f64; n]);
    let corners = |k: usize| -> Vec<usize> { mesh.corners(k).iter().map(|c| *c as usize).filter(|c| *c < n).collect() };
    let mut edges: std::collections::HashMap<(usize, usize), u32> = std::collections::HashMap::new();
    for k in 0..mesh.elements.len() {
        let c = corners(k);
        if c.len() < 3 {
            continue;
        }
        let face = (mid[c[1]] - mid[c[0]]).cross(mid[c[c.len() - 1]] - mid[c[0]]);
        for (at, &node) in c.iter().enumerate() {
            normal[node] += face;
            thick[node] = thick[node].max(mesh.thickness.get(k).copied().unwrap_or(0.0));
            let next = c[(at + 1) % c.len()];
            *edges.entry((node.min(next), node.max(next))).or_default() += 1;
        }
    }
    let up: Vec<DVec3> = normal.iter().map(|v| v.normalize_or_zero()).collect();
    let light = LIGHT.normalize();
    let tint = |node: usize, toward: DVec3| -> [u8; 4] {
        let base = scale_colour(shown.on_scale((value.values.get(node).copied().unwrap_or(low) - low) / span));
        let lit = 0.6 + 0.4 * toward.normalize_or_zero().dot(light).abs();
        [(f64::from(base[0]) * lit) as u8, (f64::from(base[1]) * lit) as u8, (f64::from(base[2]) * lit) as u8, 255]
    };
    let top = |node: usize| mid[node] + up[node] * (thick[node] * 0.5);
    let bottom = |node: usize| mid[node] - up[node] * (thick[node] * 0.5);
    let mut push = |pen: &mut Pen, p: [DVec3; 3], c: [[u8; 4]; 3]| {
        if p.iter().all(|q| q.is_finite()) {
            pen.faces.push((p.map(|q| (pen.at)(q)), c));
        }
    };
    for k in 0..mesh.elements.len() {
        let c = corners(k);
        for third in 2..c.len() {
            let (a, b, d) = (c[0], c[third - 1], c[third]);
            push(pen, [top(a), top(b), top(d)], [tint(a, up[a]), tint(b, up[b]), tint(d, up[d])]);
            push(pen, [bottom(a), bottom(d), bottom(b)], [tint(a, up[a]), tint(d, up[d]), tint(b, up[b])]);
        }
        if shown.plate_mesh {
            for at in 0..c.len() {
                let (a, b) = (c[at], c[(at + 1) % c.len()]);
                // a hair above the face, so that the line is not lost in it
                let lift = |node: usize| top(node) + up[node] * (thick[node] * 0.01 + 1e-4);
                if a < b || edges.get(&(b, a)).copied().unwrap_or(0) < 2 {
                    pen.line(lift(a), lift(b), MESH_LINE);
                }
            }
        }
    }
    // the edge of the plates: where an edge is of one element only
    for (&(a, b), &count) in &edges {
        if count != 1 {
            continue;
        }
        let side = (mid[b] - mid[a]).cross(up[a] + up[b]);
        let (ca, cb) = (tint(a, side), tint(b, side));
        push(pen, [top(a), top(b), bottom(b)], [ca, cb, cb]);
        push(pen, [top(a), bottom(b), bottom(a)], [ca, cb, ca]);
        pen.line(top(a), top(b), MESH_LINE);
        pen.line(bottom(a), bottom(b), MESH_LINE);
    }
    let peak = (0..n).filter(|k| value.values.get(*k).is_some_and(|v| v.is_finite())).max_by(|a, b| value.values[*a].abs().total_cmp(&value.values[*b].abs()))?;
    Some((format!("{} {}", value.name, value.written(value.values[peak])), top(peak)))
}

/// The reactions, each the force on the structure, with its size: one arrow per node, or
/// (`how`) an arrow along x, along y and along z in the colours of the axes. An arrow ends at its
/// node, as the force comes onto the structure there.
fn reactions(doc: &AnalysisModel, table: &results::NodeTable, reach: f64, how: ReactionsAs, pen: &mut Pen) -> Vec<(String, DVec3)> {
    let forces: Vec<(Id, DVec3)> = table.node.iter().enumerate().filter_map(|(k, id)| Some((*id, DVec3::new(*table.comps[0].get(k)?, *table.comps[1].get(k)?, *table.comps[2].get(k)?)))).filter(|(_, f)| f.is_finite() && f.length() > 0.0).collect();
    let mut out = Vec::new();
    match how {
        ReactionsAs::Resultant => {
            let most = forces.iter().fold(0.0_f64, |m, (_, f)| m.max(f.length()));
            for (id, f) in forces {
                let Some(node) = doc.node(id) else { continue };
                let tail = node.p - f / most * reach;
                pen.arrow(tail, node.p, REACTION);
                out.push((figure(f.length() / 1e3, "kN"), tail));
            }
        }
        ReactionsAs::Components => {
            let most = forces.iter().fold(0.0_f64, |m, (_, f)| m.max(f.abs().max_element()));
            for (id, f) in forces {
                let Some(node) = doc.node(id) else { continue };
                for (k, axis) in [DVec3::X, DVec3::Y, DVec3::Z].into_iter().enumerate() {
                    // a hundredth of the largest and less is left out
                    if f[k].abs() < most * 0.01 {
                        continue;
                    }
                    let tail = node.p - axis * (f[k] / most * reach);
                    pen.arrow(tail, node.p, ALONG[k]);
                    out.push((format!("R{} {}", ["x", "y", "z"][k], figure(f[k] / 1e3, "kN")), tail));
                }
            }
        }
    }
    out
}

/// A symbol at every supported node, as structural drawings have them, and a circle near every
/// released member end; `s` is the symbol's size.
///
/// - all three translations held, no rotation: a hinge, a cone with its point at the node on the
///   ground;
/// - a rotation held too: a clamp, the ground's plate at the node itself;
/// - one or two translations held: the cone on rollers, which roll the way the node is free;
/// - a spring among them: a coil between the node and the ground.
///
/// The ground is a thin plate with the strokes of hatching under it. It lies under the node, or
/// on the side the support holds when it does not hold the height.
fn supports(doc: &AnalysisModel, s: f64, pen: &mut Pen) {
    for node in &doc.nodes {
        let Some(support) = &node.support else { continue };
        let on = |k: usize| !matches!(support.dofs[k], Dof::Free);
        let (held, turn) = ([on(0), on(1), on(2)], on(3) || on(4) || on(5));
        let count = held.iter().filter(|h| **h).count();
        if count == 0 && !turn {
            continue;
        }
        let spring = support.dofs.iter().any(|d| matches!(d, Dof::Spring(_) | Dof::Nonlinear { .. }));
        let down = if held[2] || count == 0 { -DVec3::Z } else if held[0] { -DVec3::X } else { -DVec3::Y };
        // across the ground: first the way the node is free, where it is
        let across = [DVec3::X, DVec3::Y, DVec3::Z].into_iter().enumerate().filter(|(_, a)| a.dot(down) == 0.0);
        let u = across.clone().find(|(k, _)| !held[*k]).or(across.clone().next()).map_or(DVec3::X, |(_, a)| a);
        let v = down.cross(u);
        let p = node.p;
        // a stroke of hatching: a thin bar from `from`, slanting away from the node and `back`
        let stroke = |pen: &mut Pen, from: DVec3, back: DVec3| {
            let to = from + (down * 0.5 - back * 0.36) * s;
            pen.line(from, to, SUPPORT_EDGE);
            pen.round(from, to, s * 0.028, s * 0.028, SUPPORT_EDGE);
        };
        // the ground with its top at `top`: a dark plate with the hatching under its rim
        let ground = |pen: &mut Pen, top: DVec3, r: f64| {
            let under = top + down * (s * 0.12);
            pen.round(top, under, r, r, SUPPORT_EDGE);
            const N: usize = 12;
            for k in 0..N {
                let t = k as f64 / N as f64 * std::f64::consts::TAU;
                stroke(pen, under + (u * t.cos() + v * t.sin()) * (r * 0.96), u);
            }
        };
        if spring {
            // a zigzag between the node and the ground
            let n = 8;
            let mut last = p;
            for k in 1..=n {
                let side = if k == n { 0.0 } else if k % 2 == 1 { 0.4 } else { -0.4 };
                let next = p + down * (s * 1.5 * k as f64 / n as f64) + u * (side * s);
                pen.line(last, next, SUPPORT);
                pen.round(last, next, s * 0.045, s * 0.045, SUPPORT);
                last = next;
            }
            ground(pen, p + down * (s * 1.5), s * 1.1);
        } else if turn && (count == 3 || count == 0) {
            // a clamp: what the member ends in, hatched on all four sides
            let half = s * 1.25;
            pen.block(p, u, v, -down, half, s * 0.2, SUPPORT_EDGE);
            let under = p + down * (s * 0.2);
            for k in 0..=4 {
                let t = (k as f64 / 2.0 - 1.0) * half * 0.96;
                stroke(pen, under - v * half + u * t, u);
                stroke(pen, under + v * half + u * t, u);
                stroke(pen, under - u * half + v * t, v);
                stroke(pen, under + u * half + v * t, v);
            }
        } else {
            let foot = p + down * (s * 1.3);
            pen.round(p, foot, 0.0, s * 0.75, SUPPORT);
            if count == 3 {
                ground(pen, foot, s * 1.15);
            } else {
                // a roller: the cone on its own plate, three rollers lying across the way they
                // roll, and the ground under them
                pen.round(foot, foot + down * (s * 0.1), s * 0.95, s * 0.95, SUPPORT);
                for at in [-0.55, 0.0, 0.55] {
                    let c = foot + down * (s * 0.29) + u * (at * s);
                    pen.round(c - v * (s * 0.75), c + v * (s * 0.75), s * 0.19, s * 0.19, SUPPORT);
                }
                ground(pen, foot + down * (s * 0.48), s * 1.15);
            }
        }
    }
    for member in &doc.members {
        let Some(f) = frame_of(doc, member) else { continue };
        for (end, release) in member.releases.iter().enumerate() {
            if release.is_none() {
                continue;
            }
            // a little way in from the end, so that the circles of the members at a node stay apart
            let inset = (s * 2.5).min(f.length * 0.25);
            let c = if end == 0 { f.origin + f.x * inset } else { f.origin + f.x * (f.length - inset) };
            let r = s * 0.7;
            let around = |k: usize, u: DVec3, v: DVec3| {
                let t = k as f64 / 12.0 * std::f64::consts::TAU;
                c + (u * t.cos() + v * t.sin()) * r
            };
            for k in 0..12 {
                pen.line(around(k, f.x, f.z), around(k + 1, f.x, f.z), RELEASE);
                pen.line(around(k, f.x, f.y), around(k + 1, f.x, f.y), RELEASE);
            }
        }
    }
}

/// The loads of a load case: arrows at nodes, along members and on the corners of surfaces.
fn loads(doc: &AnalysisModel, case: Id, reach: f64, pen: &mut Pen) {
    let actions: Vec<&Action> = doc.actions.iter().filter(|a| a.load == LoadRef::Case(case)).collect();
    // each kind of load to its own largest
    let size = |a: &Action| match &a.kind {
        Load::NodalForce { f, .. } => (0, f.length()),
        Load::MemberPoint { f, .. } => (0, f.length()),
        Load::MemberDistributed { dist, .. } => (1, intensity(dist)),
        Load::Surface { dist, .. } => (2, match dist {
            SurfaceDistribution::Uniform(q) => q.abs(),
            SurfaceDistribution::Bilinear(q) => q.iter().fold(0.0_f64, |m, v| m.max(v.abs())),
            SurfaceDistribution::Sampled(q) => q.iter().fold(0.0_f64, |m, v| m.max(v.1.abs())),
        }),
        _ => (3, 0.0),
    };
    let mut most = [0.0_f64; 4];
    for a in &actions {
        let (kind, value) = size(a);
        most[kind] = most[kind].max(value);
    }
    for a in actions {
        let (kind, value) = size(a);
        if value <= 0.0 || most[kind] <= 0.0 {
            continue;
        }
        let length = reach * (0.35 + 0.65 * value / most[kind]);
        match (&a.kind, &a.target) {
            (Load::NodalForce { f, .. }, Target::Node(id)) => {
                if let Some(node) = doc.node(*id) {
                    pen.arrow(node.p - f.normalize_or_zero() * length, node.p, LOAD);
                }
            }
            (Load::MemberPoint { x, f, .. }, Target::Member { id, .. }) => {
                if let Some(frame) = doc.member(*id).and_then(|m| frame_of(doc, m)) {
                    let dir = if a.frame == LoadFrame::Local { frame.vector_to_global(*f) } else { *f }.normalize_or_zero();
                    let tip = frame.origin + frame.x * *x;
                    pen.arrow(tip - dir * length, tip, LOAD);
                }
            }
            (Load::MemberDistributed { dir, moment: false, .. }, Target::Member { id, span }) => {
                let Some(frame) = doc.member(*id).and_then(|m| frame_of(doc, m)) else { continue };
                let dir = if a.frame == LoadFrame::Local { frame.vector_to_global(*dir) } else { *dir }.normalize_or_zero();
                let (from, to) = span.unwrap_or((0.0, frame.length));
                // an arrow about every reach, at least at both ends
                let n = (((to - from) / reach.max(1e-6)).ceil() as usize).clamp(2, 24);
                let foot = |k: usize| frame.origin + frame.x * (from + (to - from) * k as f64 / n as f64);
                for k in 0..=n {
                    pen.arrow(foot(k) - dir * length, foot(k), LOAD);
                }
                pen.line(foot(0) - dir * length, foot(n) - dir * length, LOAD);
            }
            (Load::Surface { dir, .. }, Target::Surface(id)) => {
                let Some(surface) = doc.surface(*id) else { continue };
                let corners: Vec<DVec3> = surface.boundary.iter().filter_map(|n| doc.node(*n)).map(|n| n.p).collect();
                if corners.len() < 3 {
                    continue;
                }
                let dir = dir.normalize_or_zero();
                let middle = corners.iter().sum::<DVec3>() / corners.len() as f64;
                // on the middle and halfway to each corner, under an outline of the surface
                for &corner in &corners {
                    let tip = (middle + corner) / 2.0;
                    pen.arrow(tip - dir * length, tip, LOAD);
                }
                pen.arrow(middle - dir * length, middle, LOAD);
                for (k, &corner) in corners.iter().enumerate() {
                    let next = corners[(k + 1) % corners.len()];
                    pen.line((middle + corner) / 2.0 - dir * length, (middle + next) / 2.0 - dir * length, LOAD);
                }
            }
            _ => {}
        }
    }
}

/// The largest intensity of a member load.
fn intensity(dist: &Distribution) -> f64 {
    match dist {
        Distribution::Uniform(q) => q.abs(),
        Distribution::Linear { q1, q2 } | Distribution::Trapezoidal { q1, q2, .. } => q1.abs().max(q2.abs()),
        Distribution::Polygonal(points) => points.iter().fold(0.0_f64, |m, p| m.max(p.1.abs())),
        Distribution::Parabolic { peak } | Distribution::Sine { peak } => peak.abs(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A beam of 4 m along x on two supports, with a point load, a line load and the results of a
    /// load case: a bending moment that peaks in the middle, reactions and a sag.
    fn beam() -> Vec<StructuralPart> {
        let mut m = AnalysisModel::new(1, "beam", SourceInfo::default());
        let node = |k: u64, x: f64| Node { id: sam_id(IdKind::Node, k), number: k.to_string(), p: DVec3::new(x, 0.0, 0.0), frame: None, support: None, origin: NodeOrigin::Analytical };
        m.nodes = vec![node(1, 0.0), node(2, 4.0)];
        let held = [Dof::Fixed, Dof::Fixed, Dof::Fixed, Dof::Free, Dof::Free, Dof::Free];
        m.nodes[0].support = Some(Support { preset: support::classify(&held, Dimension::Space3d), dofs: held, frame: DofFrame::Global, named: None });
        let member = sam_id(IdKind::Member, 1);
        m.members.push(Member { id: member, number: "1".into(), kind: MemberKind::Beam, nodes: vec![m.nodes[0].id, m.nodes[1].id], geometry: Curve::Line, z_ref: DVec3::Z, orientation: Orientation::Default, section: None, section_rotation: 0.0, insertion: Insertion::default(), releases: [None, None], material: None, links: Vec::new() });
        m.members[0].releases[1] = Some(Hinge { dofs: [Dof::Fixed, Dof::Fixed, Dof::Fixed, Dof::Fixed, Dof::Free, Dof::Free], named: None });
        let case = sam_id(IdKind::LoadCase, 1);
        m.load_cases.push(LoadCase { id: case, number: "1".into(), name: "Live".into(), action_type: ActionType::Variable, action_source: String::new(), self_weight: None, coefficient: 1.0 });
        let act = |k: u64, target: Target, kind: Load| Action { id: sam_id(IdKind::Action, k), load: LoadRef::Case(case), target, frame: LoadFrame::Global, kind, destabilising: false };
        m.actions = vec![
            act(1, Target::Member { id: member, span: None }, Load::MemberDistributed { dir: DVec3::NEG_Z, moment: false, dist: Distribution::Uniform(5_000.0) }),
            act(2, Target::Node(m.nodes[1].id), Load::NodalForce { f: DVec3::new(1_000.0, 0.0, 0.0), m: DVec3::ZERO }),
        ];
        let mut comps: [Vec<f64>; 6] = Default::default();
        for c in comps.iter_mut() {
            *c = vec![0.0; 3];
        }
        comps[Force::My.slot()] = vec![0.0, 10_000.0, 0.0];
        comps[Force::Vz.slot()] = vec![10_000.0, 0.0, -10_000.0];
        let forces = results::MemberTable { member: vec![member], offsets: vec![0, 3], x: vec![0.0, 2.0, 4.0], comps };
        let table = |values: [[f64; 2]; 3]| results::NodeTable { node: vec![m.nodes[0].id, m.nodes[1].id], comps: [values[0].to_vec(), values[1].to_vec(), values[2].to_vec(), vec![0.0; 2], vec![0.0; 2], vec![0.0; 2]] };
        let payload = results::ResultPayload { member_forces: Some(forces), node_displacements: Some(table([[0.0, 0.0], [0.0, 0.0], [0.0, -0.002]])), reactions: Some(table([[0.0, 0.0], [0.0, 0.0], [10_000.0, 10_000.0]])), ..Default::default() };
        m.results.push(ResultSet { id: sam_id(IdKind::ResultSet, 1), number: "1".into(), name: "Live".into(), for_: ResultFor::Case(case), theory: Theory::First, linear: true, available: Vec::new(), inline: Some(payload) });
        m.rebuild_index();
        vec![StructuralPart::new(0, Arc::new(m))]
    }

    fn drawn(shown: Shown) -> (Vec<Colored>, Vec<(String, [f32; 3])>) {
        draw(&beam(), &[Origin::default()], &shown)
    }

    #[test]
    fn nothing_is_drawn_until_something_is_shown() {
        assert!(drawn(Shown::default()).0.is_empty());
    }

    #[test]
    fn a_bending_moment_lies_on_the_side_it_stretches_and_names_its_peak() {
        let (lines, labels) = drawn(Shown { force: Some(Force::My), scale: 1.0, ..Shown::default() });
        // the model is 4 m: the peak of 10 kNm reaches 0.48 m up the local z, in the middle
        let top = lines.iter().flat_map(|(l, _)| l.iter()).map(|p| p[2]).fold(f32::MIN, f32::max);
        assert!((top - 0.48).abs() < 1e-4, "{top}");
        assert_eq!(labels.len(), 1);
        assert_eq!((labels[0].0.as_str(), labels[0].1), ("My 10.0 kNm", [2.0, 0.0, 0.48]));
        // positive values are drawn in one colour
        assert!(lines.iter().all(|(_, c)| *c == POSITIVE));
    }

    #[test]
    fn a_shear_force_changes_sign_and_colour() {
        let (lines, labels) = drawn(Shown { force: Some(Force::Vz), scale: 1.0, ..Shown::default() });
        assert!(lines.iter().any(|(_, c)| *c == POSITIVE) && lines.iter().any(|(_, c)| *c == NEGATIVE));
        assert!(labels[0].0.starts_with("Vz ") && labels[0].0.ends_with(" kN"), "{labels:?}");
        // twice as large with twice the scale
        let (larger, _) = drawn(Shown { force: Some(Force::Vz), scale: 2.0, ..Shown::default() });
        let reach = |l: &[Colored]| l.iter().flat_map(|(l, _)| l.iter()).map(|p| p[2].abs()).fold(0.0, f32::max);
        assert!((reach(&larger) - 2.0 * reach(&lines)).abs() < 1e-4);
    }

    #[test]
    fn the_deformed_shape_reactions_and_loads_are_drawn() {
        let (lines, labels) = drawn(Shown { deformed: true, scale: 1.0, ..Shown::default() });
        assert_eq!((lines.len(), labels[0].0.as_str()), (1, "u 2.00 mm"));
        assert!(lines[0].1 == DEFORMED && (lines[0].0[1][2] + 0.48).abs() < 1e-4, "{lines:?}");
        // an arrow per reaction, pointing up into the node: its centre line here, its shaft and
        // head among the faces
        let (lines, labels) = drawn(Shown { reactions: true, scale: 1.0, ..Shown::default() });
        assert_eq!((lines.len(), labels.len(), labels[0].0.as_str()), (2, 2, "10.0 kN"));
        assert!(lines.iter().all(|(_, c)| *c == REACTION));
        // per direction: the reactions here are upright, so one arrow each, along z, named so
        let (lines, labels) = drawn(Shown { reactions: true, reactions_as: ReactionsAs::Components, scale: 1.0, ..Shown::default() });
        assert_eq!((lines.len(), labels[0].0.as_str()), (2, "Rz 10.0 kN"));
        assert!(lines.iter().all(|(_, c)| *c == ALONG[2]));
        // the line load as a row of arrows under their top line, and the point load at its node
        let (lines, _) = drawn(Shown { loads: Some(0), scale: 1.0, ..Shown::default() });
        assert!(lines.len() > 15 && lines.iter().all(|(_, c)| *c == LOAD), "{}", lines.len());
        assert!(drawn(Shown { loads: Some(7), scale: 1.0, ..Shown::default() }).0.is_empty());
        // the symbols of the supports, alone and of one size whatever the scale
        let (symbols, labels) = drawn(Shown { supports: true, scale: 1.0, ..Shown::default() });
        assert!(!symbols.is_empty() && labels.is_empty() && symbols.iter().all(|(_, c)| *c == SUPPORT || *c == SUPPORT_EDGE || *c == RELEASE));
        assert_eq!(drawn(Shown { supports: true, scale: 3.0, ..Shown::default() }).0, symbols);
    }

    #[test]
    fn arrows_supports_and_the_deformed_profile_are_solid() {
        let all = |shown: Shown| draw_all(&beam(), &[Origin::default()], &shown);
        // an arrow has a shaft and a head; a hinge is a cone on the plate of the ground
        assert!(all(Shown { reactions: true, scale: 1.0, ..Shown::default() }).faces.len() >= 2 * 40);
        assert!(all(Shown { supports: true, scale: 1.0, ..Shown::default() }).faces.len() >= 4 + 12);
        // the beam has no section: its deformed shape is a line; with a tube it is a solid too,
        // unless lines are asked for
        assert!(all(Shown { deformed: true, scale: 1.0, ..Shown::default() }).faces.is_empty());
        let mut parts = beam();
        let mut doc = (*parts[0].doc).clone();
        let section = sam_id(IdKind::Section, 1);
        let shape = Shape::Chs { d: 0.1, t: 0.005 };
        let (props, props_source) = section::compute(&shape, None);
        doc.sections.push(Section { id: section, name: "CHS 100x5".into(), shape, props, props_source, imported: None, material: None });
        doc.members[0].section = Some((section, None));
        doc.rebuild_index();
        parts[0] = StructuralPart::new(0, Arc::new(doc));
        let solid = draw_all(&parts, &[Origin::default()], &Shown { deformed: true, scale: 1.0, ..Shown::default() });
        assert!(solid.faces.len() > 40 && solid.faces.iter().all(|(_, c)| c[0][1] > c[0][0] && c[0][1] > c[0][2]), "green, shaded");
        // the free end went down by the reach: the solid's lowest point follows it
        let lowest = solid.faces.iter().flat_map(|(t, _)| t.iter().map(|p| p[2])).fold(f32::MAX, f32::min);
        assert!((lowest + (4.0 * REACH) as f32 + 0.05).abs() < 0.01, "{lowest}");
        let lines = draw_all(&parts, &[Origin::default()], &Shown { deformed: true, scale: 1.0, deformed_as: DeformedAs::Lines, ..Shown::default() });
        assert!(lines.faces.is_empty() && lines.lines == solid.lines);
        // figures can be left out; what is shown is given as it is to be
        assert!(!solid.labels.is_empty() && draw_all(&parts, &[Origin::default()], &Shown { deformed: true, scale: 1.0, hide_values: true, ..Shown::default() }).labels.is_empty());
        let mut shown = Shown::default();
        for change in [Change::ForceIs(Some(Force::Mz)), Change::DeformedOn(true), Change::ReactionsOn(true), Change::SupportsOn(true), Change::NodesOn(false), Change::MembersOn(false), Change::ValuesOn(false)] {
            shown.apply(change, 1, 1);
        }
        assert_eq!((shown.force, shown.deformed, shown.reactions, shown.supports, shown.hide_nodes, shown.hide_members, shown.hide_values), (Some(Force::Mz), true, true, true, true, true, true));
    }

    /// A square plate of 2 m and 0.2 m thick as two elements (a quadrilateral and a triangle
    /// beside it), its far corner 10 mm down, with a field that runs from 0 to 5 kNm/m.
    fn plate() -> Vec<StructuralPart> {
        let mut m = AnalysisModel::new(1, "plate", SourceInfo::default());
        let surface = sam_id(IdKind::Surface, 1);
        let points = vec![DVec3::ZERO, DVec3::new(2.0, 0.0, 0.0), DVec3::new(2.0, 2.0, 0.0), DVec3::new(0.0, 2.0, 0.0), DVec3::new(3.0, 1.0, 0.0)];
        let mut displacements = vec![DVec3::ZERO; 5];
        displacements[2] = DVec3::new(0.0, 0.0, -0.01);
        let mesh = results::SurfaceTable {
            points,
            displacements,
            elements: vec![[0, 1, 2, 3], [1, 4, 2, 2]],
            surface: vec![surface; 2],
            thickness: vec![0.2; 2],
            fields: vec![results::SurfaceField { name: "mx".into(), unit: "N·m/m".into(), values: vec![0.0, 1000.0, 5000.0, 2000.0, 3000.0] }],
        };
        let payload = results::ResultPayload { surface_mesh: Some(mesh), ..Default::default() };
        m.results.push(ResultSet { id: sam_id(IdKind::ResultSet, 1), number: "1".into(), name: "Live".into(), for_: ResultFor::Envelope("all".into()), theory: Theory::First, linear: true, available: Vec::new(), inline: Some(payload) });
        m.rebuild_index();
        vec![StructuralPart::new(0, Arc::new(m))]
    }

    #[test]
    fn a_plate_is_a_volume_in_the_colours_of_its_value() {
        let all = |shown: Shown| draw_all(&plate(), &[Origin::default()], &shown);
        assert!(all(Shown { scale: 1.0, ..Shown::default() }).faces.is_empty());
        // a top and a bottom of three triangles each, and two per edge around the two elements
        let drawn = all(Shown { plates: true, plate_value: 1, scale: 1.0, ..Shown::default() });
        assert_eq!((drawn.faces.len(), drawn.labels[0].0.as_str()), (3 * 2 + 5 * 2, "mx 5.00 kNm/m"));
        // at rest its faces lie half its thickness above and under its plane
        let heights = |d: &Drawing| d.faces.iter().flat_map(|(t, _)| t.iter().map(|p| p[2])).fold((f32::MAX, f32::MIN), |(lo, hi), z| (lo.min(z), hi.max(z)));
        assert_eq!(heights(&drawn), (-0.1, 0.1));
        // the lowest value is blue, the highest red, and a face runs from the one to the other
        let colours: Vec<[u8; 4]> = drawn.faces.iter().flat_map(|(_, c)| *c).collect();
        assert!(colours.iter().any(|c| i32::from(c[2]) > i32::from(c[0]) + 40) && colours.iter().any(|c| i32::from(c[0]) > i32::from(c[2]) + 80), "{colours:?}");
        assert!(drawn.faces.iter().any(|(_, c)| c[0] != c[1] || c[1] != c[2]), "a gradient over a face");
        assert_eq!((scale_colour(0.0), scale_colour(1.0), scale_colour(f64::NAN)), ([43, 131, 186], [215, 25, 28], [43, 131, 186]));
        // the displacement: the corner that went down is the largest, and with the deformed shape
        // it goes down by the reach
        let moved = all(Shown { plates: true, deformed: true, scale: 1.0, ..Shown::default() });
        assert_eq!(moved.labels[0].0, "u 10.0 mm");
        // (the model has no nodes of its own, so its size counts as one metre)
        let reach = REACH as f32;
        assert!((heights(&moved).0 + reach + 0.1).abs() < 0.02, "{:?} {reach}", heights(&moved));
        // the lines of the mesh, and of the edge
        let lines = |mesh: bool| all(Shown { plates: true, plate_mesh: mesh, scale: 1.0, ..Shown::default() }).lines.len();
        assert_eq!((lines(false), lines(true)), (5 * 2, 5 * 2 + 6));
        let mut shown = Shown::default();
        for change in [Change::Plates, Change::PlateValue(1), Change::PlateMeshOn(true)] {
            shown.apply(change, 0, 1);
        }
        assert!(shown.plates && shown.plate_mesh && shown.plate_value == 1 && shown.anything());
    }

    #[test]
    fn stresses_follow_from_the_moments_and_the_thickness() {
        let parts = plate();
        let mesh = parts[0].doc.results[0].inline.as_ref().unwrap().surface_mesh.as_ref().unwrap();
        // the displacement, the one field, and six stresses at the top and at the bottom
        let names = plate_values(mesh);
        assert_eq!((names.len(), names[2].as_str(), names[13].as_str()), (2 + 12, "σx top", "σvm bottom"));
        // 5 kNm/m in a plate of 0.2 m: 6 m / t² = 0.75 MPa, pulling at the bottom, pushing at the top
        let (top, bottom) = (plate_value(mesh, 2), plate_value(mesh, 8));
        assert_eq!((top.name.as_str(), top.unit, bottom.name.as_str()), ("σx top", "MPa", "σx bottom"));
        assert!((top.values[2] + 750_000.0).abs() < 1e-6 && (bottom.values[2] - 750_000.0).abs() < 1e-6, "{:?}", top.values);
        assert_eq!(bottom.written(bottom.values[2]), "0.75 MPa");
        // with one stress only, it is the first principal one and the equivalent one too
        assert!((plate_value(mesh, 11).values[2] - 750_000.0).abs() < 1e-6 && (plate_value(mesh, 13).values[2] - 750_000.0).abs() < 1e-6);
        // a value that is not there is the displacement
        assert_eq!(plate_value(mesh, 99).name, "u");
    }

    #[test]
    fn sizes_and_colours_are_set_apart_and_kept_by_a_view() {
        let mut shown = Shown { scale: 1.0, ..Shown::default() };
        assert_eq!((shown.size(Part::Loads), shown.colours(), shown.on_scale(0.25)), (1.0, (0.0, 1.0), 0.25));
        for change in [Change::Size(Part::Reactions, 2.0), Change::Size(Part::Symbols, 1000.0), Change::ColourFrom(0.5), Change::ColourTo(0.2), Change::Bands(4)] {
            shown.apply(change, 0, 1);
        }
        // a size stays within bounds; the colours keep a part of the range between their ends
        assert_eq!((shown.size(Part::Reactions), shown.size(Part::Symbols), shown.size(Part::Forces)), (2.0, 20.0, 1.0));
        assert_eq!(shown.colours(), (0.5, 0.55));
        // under where the colours start it is the first band, over where they end the last
        assert_eq!((shown.on_scale(0.1), shown.on_scale(0.9)), (0.125, 0.875));
        // the reactions twice as long, the other arrows as they were
        let length = |shown: Shown| drawn(Shown { reactions: true, ..shown }).0.iter().map(|(l, _)| (DVec3::from(l[1].map(f64::from)) - DVec3::from(l[0].map(f64::from))).length()).fold(0.0_f64, f64::max);
        assert!((length(shown) / length(Shown { scale: 1.0, ..Shown::default() }) - 2.0).abs() < 1e-6);
        // a view keeps all of it
        shown.apply(Change::ForceIs(Some(Force::Vz)), 0, 1);
        shown.apply(Change::ReactionsAs(ReactionsAs::Components), 0, 1);
        shown.apply(Change::PlatesOn(true), 0, 1);
        assert_eq!(Shown::from_json(&shown.to_json()), Some(shown));
        assert_eq!(Shown::from_json(&serde_json::json!({})), Some(Shown::default()));
        assert_eq!(Shown::from_json(&serde_json::json!(3)), None);
    }

    #[test]
    fn a_force_line_is_drawn_with_or_without_strokes_and_named_per_member() {
        let with = drawn(Shown { force: Some(Force::My), scale: 1.0, ..Shown::default() });
        let without = drawn(Shown { force: Some(Force::My), scale: 1.0, style: Style::Outline, ..Shown::default() });
        // the stroke at the middle station is left out; the peak is named as before
        assert_eq!((with.0.len() - without.0.len(), &without.1), (1, &with.1));
        let per_member = drawn(Shown { force: Some(Force::My), scale: 1.0, values: Values::Members, ..Shown::default() });
        assert_eq!(per_member.1, with.1);
        // a place or scale that is given takes effect at once
        let mut shown = Shown { scale: 1.0, ..Shown::default() };
        shown.apply(Change::SetAt(5), 3, 2);
        shown.apply(Change::CaseAt(Some(2)), 3, 2);
        shown.apply(Change::ScaleTo(2.5), 3, 2);
        assert_eq!((shown.set, shown.loads, shown.scale), (1, Some(2), 2.5));
        shown.apply(Change::CaseAt(Some(9)), 3, 2);
        shown.apply(Change::ScaleTo(f32::NAN), 3, 2);
        assert_eq!((shown.loads, shown.scale), (None, 2.5));
    }
}
