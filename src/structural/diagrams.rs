//! What a structural analysis model shows of its loads and results in the 3D view, as lines
//! over the model and a few figures beside them.
//!
//! - **Internal forces**: one of the six (N, Vy, Vz, Mx, My, Mz) of a result set, drawn along
//!   every member as a line off its axis with a stroke at each station. N, Vz, Mx and My are set
//!   out along the member's local z, Vy and Mz along its local y; a bending moment lies on the
//!   side of the fibre it stretches (+My stretches +z, +Mz stretches −y). The largest value of
//!   the model takes [`REACH`] of the model's size.
//! - **Deformed shape**: the members between their nodes as displaced, the largest displacement
//!   taking [`REACH`] of the model's size.
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
const REACTION: [u8; 4] = [124, 58, 237, 255];
const LOAD: [u8; 4] = [217, 119, 6, 255];
const SUPPORT: [u8; 4] = [15, 118, 110, 255];
const RELEASE: [u8; 4] = [219, 39, 119, 255];
/// The size of a support symbol, as a part of the model's size; it does not follow the scale.
const SYMBOL: f64 = 0.014;

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
            Change::Style(style) => self.style = style,
            Change::Values(values) => self.values = values,
            Change::Scale(larger) => self.scale = (if self.scale > 0.0 { self.scale } else { 1.0 } * if larger { 1.5 } else { 1.0 / 1.5 }).clamp(0.05, 20.0),
        }
    }

    pub fn anything(&self) -> bool {
        self.force.is_some() || self.deformed || self.reactions || self.supports || self.loads.is_some()
    }
}

/// The lines and figures of `shown` for `parts`, in display coordinates.
pub fn draw(parts: &[StructuralPart], origins: &[Origin], shown: &Shown) -> (Vec<Colored>, Vec<(String, [f32; 3])>) {
    let (mut lines, mut labels) = (Vec::new(), Vec::new());
    if !shown.anything() {
        return (lines, labels);
    }
    for part in parts {
        let doc = &part.doc;
        let origin = origins.get(part.model as usize).copied().unwrap_or_default();
        let at = |p: DVec3| origin.display(p).as_vec3().to_array();
        // the model's size: the diagonal of its nodes
        let (low, high) = doc.nodes.iter().fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(lo, hi), n| (lo.min(n.p), hi.max(n.p)));
        let size = if low.cmple(high).all() { (high - low).length().max(1.0) } else { 1.0 };
        let reach = size * REACH * f64::from(if shown.scale > 0.0 { shown.scale } else { 1.0 });
        let mut pen = Pen { lines: &mut lines, at: &at };
        let results = doc.results.get(shown.set).and_then(|r| r.inline.as_ref());
        if let (Some(force), Some(table)) = (shown.force, results.and_then(|r| r.member_forces.as_ref())) {
            for (value, p) in forces(doc, table, force, reach, shown, &mut pen) {
                labels.push((format!("{} {}", force.symbol(), figure(value / 1e3, force.unit())), at(p)));
            }
        }
        if let (true, Some(table)) = (shown.deformed, results.and_then(|r| r.node_displacements.as_ref())) {
            if let Some((value, p)) = deformed(doc, table, reach, &mut pen) {
                labels.push((format!("u {}", figure(value * 1e3, "mm")), at(p)));
            }
        }
        if let (true, Some(table)) = (shown.reactions, results.and_then(|r| r.reactions.as_ref())) {
            for (value, p) in reactions(doc, table, reach, &mut pen) {
                labels.push((figure(value / 1e3, "kN"), at(p)));
            }
        }
        if let Some(case) = shown.loads.and_then(|k| doc.load_cases.get(k)) {
            loads(doc, case.id, reach * 0.6, &mut pen);
        }
        if shown.supports {
            supports(doc, size * SYMBOL, &mut pen);
        }
    }
    (lines, labels)
}

/// A figure with its unit, in three significant digits.
fn figure(value: f64, unit: &str) -> String {
    let digits = if value.abs() >= 100.0 { 0 } else if value.abs() >= 10.0 { 1 } else { 2 };
    format!("{value:.digits$} {unit}")
}

/// Draws lines in display coordinates from world points.
struct Pen<'a> {
    lines: &'a mut Vec<Colored>,
    at: &'a dyn Fn(DVec3) -> [f32; 3],
}

impl Pen<'_> {
    fn line(&mut self, a: DVec3, b: DVec3, color: [u8; 4]) {
        if a.is_finite() && b.is_finite() && a != b {
            self.lines.push(([(self.at)(a), (self.at)(b)], color));
        }
    }

    /// An arrow from `tail` to `tip`, its head a fifth of its length.
    fn arrow(&mut self, tail: DVec3, tip: DVec3, color: [u8; 4]) {
        let shaft = tip - tail;
        let length = shaft.length();
        if length <= 1e-9 || !length.is_finite() {
            return;
        }
        self.line(tail, tip, color);
        let along = shaft / length;
        let side = frame::default_z_ref(along).cross(along).normalize_or_zero();
        let up = along.cross(side);
        for wing in [side, -side, up, -up] {
            self.line(tip, tip - along * length * 0.22 + wing * length * 0.09, color);
        }
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

/// The members between their displaced nodes; the largest displacement and where it is.
fn deformed(doc: &AnalysisModel, table: &results::NodeTable, reach: f64, pen: &mut Pen) -> Option<(f64, DVec3)> {
    let moved: std::collections::HashMap<Id, DVec3> = table.node.iter().enumerate().filter_map(|(k, id)| Some((*id, DVec3::new(*table.comps[0].get(k)?, *table.comps[1].get(k)?, *table.comps[2].get(k)?)))).filter(|(_, u)| u.is_finite()).collect();
    let most = moved.values().fold(0.0_f64, |m, u| m.max(u.length()));
    if most <= 0.0 {
        return None;
    }
    let place = |id: Id| doc.node(id).map(|n| n.p + moved.get(&id).copied().unwrap_or(DVec3::ZERO) * (reach / most));
    for member in &doc.members {
        if let (Some(a), Some(b)) = (member.nodes.first().and_then(|id| place(*id)), member.nodes.last().and_then(|id| place(*id))) {
            pen.line(a, b, DEFORMED);
        }
    }
    let (id, u) = moved.iter().max_by(|a, b| a.1.length().total_cmp(&b.1.length()))?;
    Some((u.length(), place(*id)?))
}

/// An arrow at every node with a reaction: the force on the structure; each with its size.
fn reactions(doc: &AnalysisModel, table: &results::NodeTable, reach: f64, pen: &mut Pen) -> Vec<(f64, DVec3)> {
    let forces: Vec<(Id, DVec3)> = table.node.iter().enumerate().filter_map(|(k, id)| Some((*id, DVec3::new(*table.comps[0].get(k)?, *table.comps[1].get(k)?, *table.comps[2].get(k)?)))).filter(|(_, f)| f.is_finite() && f.length() > 0.0).collect();
    let most = forces.iter().fold(0.0_f64, |m, (_, f)| m.max(f.length()));
    let mut out = Vec::new();
    for (id, f) in forces {
        let Some(node) = doc.node(id) else { continue };
        // the arrow ends at the node, as the force comes onto the structure there
        let tail = node.p - f / most * reach;
        pen.arrow(tail, node.p, REACTION);
        out.push((f.length(), tail));
    }
    out
}

/// A symbol at every supported node and a circle near every released member end; `s` is the
/// symbol's size. A pinned support is a pyramid with its top at the node on a hatched ground
/// plate; a roller stands on a second plate under a gap; a fixed support is the ground plate at
/// the node itself; anything else a diamond.
fn supports(doc: &AnalysisModel, s: f64, pen: &mut Pen) {
    let (x, y, z) = (DVec3::X * s, DVec3::Y * s, DVec3::Z * s);
    // a square plate around `c` with the strokes of the ground under it
    let plate = |pen: &mut Pen, c: DVec3, half: f64| {
        let corners = [c - (x + y) * half, c + (x - y) * half, c + (x + y) * half, c - (x - y) * half];
        for k in 0..4 {
            pen.line(corners[k], corners[(k + 1) % 4], SUPPORT);
        }
        for k in 0..=4 {
            let t = -half + 2.0 * half * k as f64 / 4.0;
            pen.line(c + x * t - y * half, c + x * (t - 0.35) - y * half - z * 0.45, SUPPORT);
            pen.line(c + y * t + x * half, c + y * (t - 0.35) + x * half - z * 0.45, SUPPORT);
        }
    };
    let pyramid = |pen: &mut Pen, top: DVec3, base: DVec3| {
        let corners = [base - x - y, base + x - y, base + x + y, base - x + y];
        for k in 0..4 {
            pen.line(top, corners[k], SUPPORT);
            pen.line(corners[k], corners[(k + 1) % 4], SUPPORT);
        }
    };
    for node in &doc.nodes {
        let Some(support) = &node.support else { continue };
        let p = node.p;
        match support.preset {
            SupportPreset::Fixed => {
                plate(pen, p, 1.5);
                pen.line(p - x * 1.5, p + x * 1.5, SUPPORT);
                pen.line(p - y * 1.5, p + y * 1.5, SUPPORT);
            }
            SupportPreset::Pinned => {
                pyramid(pen, p, p - z * 1.8);
                plate(pen, p - z * 1.8, 1.5);
            }
            SupportPreset::Roller { .. } | SupportPreset::Sliding { .. } => {
                pyramid(pen, p, p - z * 1.8);
                plate(pen, p - z * 2.3, 1.5);
            }
            SupportPreset::Free => {}
            SupportPreset::Spring | SupportPreset::Custom => {
                for (u, v) in [(x, z), (y, z), (x, y)] {
                    for (a, b) in [(p - u, p - v), (p - v, p + u), (p + u, p + v), (p + v, p - u)] {
                        pen.line(a, b, SUPPORT);
                    }
                }
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
        // an arrow of five lines per reaction, pointing up into the node
        let (lines, labels) = drawn(Shown { reactions: true, scale: 1.0, ..Shown::default() });
        assert_eq!((lines.len(), labels.len(), labels[0].0.as_str()), (10, 2, "10.0 kN"));
        assert!(lines.iter().all(|(_, c)| *c == REACTION));
        // the line load as a row of arrows under their top line, and the point load at its node
        let (lines, _) = drawn(Shown { loads: Some(0), scale: 1.0, ..Shown::default() });
        assert!(lines.len() > 15 && lines.iter().all(|(_, c)| *c == LOAD), "{}", lines.len());
        assert!(drawn(Shown { loads: Some(7), scale: 1.0, ..Shown::default() }).0.is_empty());
        // the symbols of the supports, alone and of one size whatever the scale
        let (symbols, labels) = drawn(Shown { supports: true, scale: 1.0, ..Shown::default() });
        assert!(!symbols.is_empty() && labels.is_empty() && symbols.iter().all(|(_, c)| *c == SUPPORT || *c == RELEASE));
        assert_eq!(drawn(Shown { supports: true, scale: 3.0, ..Shown::default() }).0, symbols);
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
