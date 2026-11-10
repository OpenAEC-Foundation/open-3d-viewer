//! The system generator: the members and panels of a system, and the type a system is made by.
//!
//! A system is a vertical face on its line ([`SystemDef`]), divided into equal bays along it and
//! rows up it. Its type, copied from a library as a build-up is, adds what a curtain wall alone
//! lacks: the pattern of its rows (a running bond shifts every second row half a bay), the material
//! of its members and of its panels, a framing of its members, and layers across the face, each
//! with a grid of its own (a timber frame's boards and membranes, its service zone). Without a type
//! a system is a plain curtain wall.
//!
//! Members on a grid are centred on its lines, the edges included, and meet as join elements do,
//! through the kernel's join geometry ([`crate::join`]). In a stacked grid the vertical members
//! run through and the horizontal ones stop against them; in a running bond the rows' lines run
//! through between the edges and the joints between the units stop against them, so a bond's
//! joints do not run through. At the face's corners the edge members meet as the type says: one
//! stopping against the other, which reaches the corner, or both mitred.
//!
//! Framed members (a timber frame's) lie inside the face, square cut: the plates run through along
//! its foot and its head, the studs stand between them, one at either end and one at every bay's
//! centre from the start, and noggins stand between the studs at every row up from the foot. A
//! line on an edge member needs no member of its own, and a stud or a noggin that would overlap an
//! edge member stands against it, so a board's seam always has timber behind it.
//!
//! A framed face frames the system's openings (a timber frame's): a king stud either side runs from
//! plate to plate, a trimmer inside it carries the header over the opening, a window has a sill
//! under it, and cripple studs stand above the header (over the trimmers too) and below the sill
//! at the bay's centres. The bottom plates stop at a door. Insulation and boards stop at the openings, a board cut around one
//! into rectangles: the strips beside it, then the pieces below and above it. Only framed systems
//! take openings.
//!
//! Panels fill the bays (or the cells of a frame) between the members, centred on their face. A
//! layer without members lays whole boards edge to edge from the face's start and foot, the last
//! ones cut to fit.

use crate::element::{Opening, PartSolid, SystemDef};
use crate::join::{self, JoinKind, Member, MemberEnd};
use crate::profile::Profile;
use crate::solid::{Axes, Plane, Solid};
use crate::{positive, Refusal, LINEAR_TOLERANCE};
use bim_library::system::{Corner, Framing, Pattern, SystemLayer, SystemType};
use bim_library::Library;
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

/// The most parts one system is generated as; a system that would have more is refused.
pub const MAX_PARTS: usize = 50_000;

/// A system's type as the system keeps it: copied from the library, so the element is whole
/// without it. The sizes of the system's own face are its `spacing`, `member` and `panel`; this
/// holds the type's id, the pattern of the face's rows, the material of its members and its
/// panels, the framing of its members, and the layers across it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemTypeDef {
    pub id: String,
    #[serde(default, skip_serializing_if = "is_stack")]
    pub pattern: Pattern,
    pub member_material: String,
    pub panel_material: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<SystemLayer>,
    #[serde(default, skip_serializing_if = "is_butt")]
    pub corners: Corner,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framing: Option<Framing>,
}

fn is_butt(c: &Corner) -> bool {
    *c == Corner::Butt
}

fn is_stack(p: &Pattern) -> bool {
    *p == Pattern::Stack
}

impl SystemDef {
    /// This system made by type `t`: its spacing, members and panels the type's, and the type kept
    /// with it. `None` when `t` lies (a floor, a ceiling), which an upright system cannot be.
    pub fn typed(mut self, t: &SystemType) -> Option<Self> {
        if t.lying.is_some() {
            return None;
        }
        self.spacing = DVec2::new(t.grid.bay, t.grid.row);
        self.member = DVec2::new(t.members.width, t.members.depth);
        self.panel = t.panels.thickness;
        self.system_type = Some(SystemTypeDef {
            id: t.id.clone(),
            pattern: t.grid.pattern,
            member_material: t.members.material.clone(),
            panel_material: t.panels.material.clone(),
            layers: t.layers.clone(),
            corners: t.corners,
            framing: t.members.framing,
        });
        Some(self)
    }

    /// This system made by type `id` of `library`; `None` when the library lacks it or it lies.
    pub fn typed_from(self, library: &Library, id: &str) -> Option<Self> {
        self.typed(library.system_type(id)?)
    }
}

/// One face of a system: its offset across the system's face (to the right of its line, its
/// exterior, positive), its grid, its members (size in the face and across it, material), how they
/// meet at its corners or how they are framed, and its panels (thickness, material).
struct Face<'a> {
    offset: f64,
    spacing: DVec2,
    pattern: Pattern,
    corners: Corner,
    member: Option<(DVec2, Option<&'a str>)>,
    framing: Option<Framing>,
    panel: (f64, Option<&'a str>),
}

impl Face<'_> {
    fn width(&self) -> f64 {
        self.member.map_or(0.0, |(m, _)| m.x)
    }

    /// The bays and rows on a face `length` long and `height` high, and their size: with members,
    /// equal ones as near the grid's as fit; without (boards), whole ones from the start and the
    /// foot, the last ones cut, which are counted with them.
    fn division(&self, length: f64, height: f64) -> (usize, usize, f64, f64) {
        // a count beyond any system's (and beyond what usize holds) is refused by the part count
        let count = |n: f64| (n.min(1e12) as usize).max(1);
        if self.member.is_none() {
            let whole = |extent: f64, pitch: f64| count(((extent - LINEAR_TOLERANCE) / pitch).ceil());
            return (whole(length, self.spacing.x), whole(height, self.spacing.y), self.spacing.x, self.spacing.y);
        }
        let (bays, rows) = (count((length / self.spacing.x).round()), count((height / self.spacing.y).round()));
        (bays, rows, length / bays as f64, height / rows as f64)
    }

    /// At most how many parts it is generated as, with `openings` openings.
    fn most_parts(&self, length: f64, height: f64, openings: usize) -> usize {
        let (bays, rows, _, _) = self.division(length, height);
        if let Some(framing) = self.framing {
            // up to three studs more than the bays and a noggin row more than the rows, and six
            // studs more for every opening (its king studs, its trimmers, a cripple against either
            // side); every stud cut in two by an opening, a noggin and a cell beside every stud in
            // every row; the plates, cut at every opening, its header and its sill
            let plates = (usize::from(framing.bottom_plates) + usize::from(framing.top_plates)).saturating_mul(openings + 1);
            let studs = bays.saturating_add(3).saturating_add(openings.saturating_mul(6));
            return studs.saturating_mul(rows.saturating_add(2)).saturating_mul(4).saturating_add(plates).saturating_add(openings.saturating_mul(2));
        }
        if self.member.is_none() {
            // every opening cuts a board into four pieces at most
            return bays.saturating_mul(rows).saturating_mul(openings.saturating_mul(3).saturating_add(1));
        }
        let cells = bays.saturating_mul(rows);
        match self.pattern {
            // the vertical members, the horizontal ones between them, the panels
            Pattern::Stack => (bays + 1).saturating_add(cells.saturating_add(bays)).saturating_add(cells),
            // the edges and the rows' lines, then per row one division and one panel more than
            // its bays at most
            Pattern::RunningBond => (rows + 3).saturating_add(rows.saturating_mul(bays + 1).saturating_mul(2)),
        }
    }

    /// Checks it: positive sizes, bays and rows wider than the members; a frame with room for its
    /// end studs and, between its plates, for its studs, and for the framing of its openings.
    fn check(&self, length: f64, height: f64, openings: &[Opening]) -> Result<(), Refusal> {
        let sizes = [self.spacing.x, self.spacing.y, self.panel.0];
        let member = self.member.map_or(Vec::new(), |(m, _)| vec![m.x, m.y]);
        if !sizes.into_iter().chain(member).all(positive) || !self.offset.is_finite() {
            return Err(Refusal::NotPositive);
        }
        if let (Some(framing), Some((m, _))) = (self.framing, self.member) {
            let plates = f64::from(framing.bottom_plates) + f64::from(framing.top_plates);
            let room = [self.spacing.x - m.x, self.spacing.y - m.x, length - 2.0 * m.x, height - plates * m.x];
            if !room.into_iter().all(|r| r > LINEAR_TOLERANCE) || !positive(framing.header) {
                return Err(Refusal::NotPositive);
            }
            return framed_openings(length, height, framing, m.x, openings);
        }
        let (_, _, bay, row) = self.division(length, height);
        if bay - self.width() <= LINEAR_TOLERANCE || row - self.width() <= LINEAR_TOLERANCE {
            return Err(Refusal::NotPositive);
        }
        Ok(())
    }
}

/// The faces of system `s`: its own, then its type's layers.
fn faces(s: &SystemDef) -> Vec<Face<'_>> {
    let t = s.system_type.as_ref();
    let own = Face {
        offset: 0.0,
        spacing: s.spacing,
        pattern: t.map_or(Pattern::Stack, |t| t.pattern),
        corners: t.map_or(Corner::Butt, |t| t.corners),
        member: Some((s.member, t.map(|t| t.member_material.as_str()))),
        framing: t.and_then(|t| t.framing),
        panel: (s.panel, t.map(|t| t.panel_material.as_str())),
    };
    let layers = t.into_iter().flat_map(|t| &t.layers).map(|l| Face {
        offset: l.offset,
        spacing: DVec2::new(l.grid.bay, l.grid.row),
        pattern: l.grid.pattern,
        corners: Corner::Butt,
        member: l.members.as_ref().map(|m| (DVec2::new(m.width, m.depth), Some(m.material.as_str()))),
        framing: l.members.as_ref().and_then(|m| m.framing),
        panel: (l.panels.thickness, Some(l.panels.material.as_str())),
    });
    std::iter::once(own).chain(layers).collect()
}

/// The members and panels of system `s`, face by face.
pub fn parts(s: &SystemDef) -> Result<Vec<PartSolid>, Refusal> {
    if !positive(s.height) {
        return Err(Refusal::NotPositive);
    }
    let d = s.end - s.start;
    let length = d.length();
    if !positive(length) {
        return Err(Refusal::ZeroLength);
    }
    let faces = faces(s);
    // openings are framed: by the system's own face and by every face with members
    if !s.openings.is_empty() && faces.iter().any(|f| f.member.is_some() && f.framing.is_none()) {
        return Err(Refusal::OpeningNotFramed);
    }
    for f in &faces {
        f.check(length, s.height, &s.openings)?;
    }
    if faces.iter().map(|f| f.most_parts(length, s.height, s.openings.len())).fold(0usize, usize::saturating_add) > MAX_PARTS {
        return Err(Refusal::TooManyParts);
    }
    let dir = (d / length).extend(0.0);
    let line = Line { origin: s.start.extend(s.base), dir, right: dir.cross(DVec3::Z), length, height: s.height };
    let mut parts = Vec::new();
    for f in &faces {
        line.generate(f, &s.openings, &mut parts)?;
    }
    Ok(parts)
}

/// A system's line: where its face starts, along it, to its right, and how long and high it is.
struct Line {
    origin: DVec3,
    dir: DVec3,
    right: DVec3,
    length: f64,
    height: f64,
}

/// A member being generated: its centre line as a join member, and the planes its joins trim its
/// ends to, with what they add to it and take out of it.
struct Piece {
    member: Member,
    start: Option<Plane>,
    end: Option<Plane>,
    additions: Vec<Solid>,
    removals: Vec<Solid>,
}

impl Line {
    /// A point of face `f`: `u` along the line, `v` up it.
    fn at(&self, f: &Face, u: f64, v: f64) -> DVec3 {
        self.origin + self.dir * u + DVec3::Z * v + self.right * f.offset
    }

    /// A vertical member of `f` at `u`, its centre line from `from` up to `to`.
    fn vertical(&self, f: &Face, size: DVec2, u: f64, from: f64, to: f64) -> Result<Piece, Refusal> {
        let member = Member { axes: Axes::new(self.at(f, u, from), self.dir, DVec3::Z)?, length: to - from, profile: Profile::rectangle(size.x, size.y)? };
        Ok(Piece { member, start: None, end: None, additions: Vec::new(), removals: Vec::new() })
    }

    /// A horizontal member of `f` at `v`, its centre line from `from` along to `to`.
    fn horizontal(&self, f: &Face, size: DVec2, v: f64, from: f64, to: f64) -> Result<Piece, Refusal> {
        let member = Member { axes: Axes::new(self.at(f, from, v), self.right, self.dir)?, length: to - from, profile: Profile::rectangle(size.y, size.x)? };
        Ok(Piece { member, start: None, end: None, additions: Vec::new(), removals: Vec::new() })
    }

    /// A panel of face `f`, `du` by `dv` from (`u`, `v`), centred on the face.
    fn panel(&self, f: &Face, u: f64, v: f64, du: f64, dv: f64) -> Result<PartSolid, Refusal> {
        let (thickness, material) = f.panel;
        let axes = Axes::new(self.at(f, u, v) - self.right * (thickness / 2.0), self.dir, self.right)?;
        let p = DVec2::new;
        let face = Profile::new(vec![p(0.0, 0.0), p(du, 0.0), p(du, dv), p(0.0, dv)], Vec::new())?;
        Ok(PartSolid { class: "IfcPlate", solid: Solid::extrude(&face, &axes, thickness)?, material: material.map(str::to_string) })
    }

    /// Framed face `f`: its plates, its end studs, its openings each between its king studs, the
    /// studs and noggins between those, and the cells between all of them, inside the face.
    fn framed(&self, f: &Face, framing: Framing, size: DVec2, material: Option<&str>, openings: &[Opening], parts: &mut Vec<PartSolid>) -> Result<(), Refusal> {
        let w = size.x;
        let (length, height) = (self.length, self.height);
        let (bottom, top) = (f64::from(framing.bottom_plates) * w, height - f64::from(framing.top_plates) * w);
        let mut openings: Vec<&Opening> = openings.iter().collect();
        openings.sort_by(|a, b| a.along.total_cmp(&b.along));
        let mut frame = Frame { line: self, face: f, size, members: Vec::new(), cells: Vec::new() };
        // the plates, the bottom ones stopping at the doors
        for k in 0..framing.bottom_plates {
            let v = (f64::from(k) + 0.5) * w;
            let mut from = 0.0;
            for o in openings.iter().filter(|o| is_door(o)) {
                frame.rail(v, from, o.along, w)?;
                from = o.along + o.width;
            }
            frame.rail(v, from, length, w)?;
        }
        for k in 0..framing.top_plates {
            frame.rail(height - (f64::from(k) + 0.5) * w, 0.0, length, w)?;
        }
        // the end studs, and between them each opening between its king studs
        frame.stud(w / 2.0, bottom, top)?;
        let mut from = w;
        for o in &openings {
            let (a, b, head) = (o.along, o.along + o.width, o.sill + o.height);
            frame.fill(from, a - 2.0 * w, bottom, top)?;
            frame.stud(a - 1.5 * w, bottom, top)?;
            frame.stud(a - 0.5 * w, bottom, head)?;
            frame.stud(b + 0.5 * w, bottom, head)?;
            frame.stud(b + 1.5 * w, bottom, top)?;
            frame.rail(head + framing.header / 2.0, a - w, b + w, framing.header)?;
            // cripples over the trimmers, so a board's seam on the jamb has timber behind it, and
            // between them at the bay's centres
            frame.stud(a - 0.5 * w, head + framing.header, top)?;
            frame.stud(b + 0.5 * w, head + framing.header, top)?;
            frame.fill(a, b, head + framing.header, top)?;
            if !is_door(o) {
                frame.rail(o.sill - w / 2.0, a, b, w)?;
                frame.fill(a, b, bottom, o.sill - w)?;
            }
            from = b + 2.0 * w;
        }
        frame.fill(from, length - w, bottom, top)?;
        frame.stud(length - w / 2.0, bottom, top)?;
        for m in &frame.members {
            parts.push(PartSolid { class: "IfcMember", solid: join::finish(&m.member, None, None, &[], &[])?, material: material.map(str::to_string) });
        }
        for c in &frame.cells {
            parts.push(self.panel(f, c[0], c[1], c[2] - c[0], c[3] - c[1])?);
        }
        Ok(())
    }

    fn generate(&self, f: &Face, openings: &[Opening], parts: &mut Vec<PartSolid>) -> Result<(), Refusal> {
        if let (Some(framing), Some((size, material))) = (f.framing, f.member) {
            return self.framed(f, framing, size, material, openings, parts);
        }
        let (bays, rows, bay, row) = f.division(self.length, self.height);
        let w = f.width();
        // a panel between the lines `w` apart, up to the face's end and head (where boards are
        // cut), and cut around the openings
        let panel = |u: f64, v: f64, du: f64, dv: f64| -> Result<Vec<PartSolid>, Refusal> {
            let r = [u + w / 2.0, v + w / 2.0, u + du.min(self.length - u) - w / 2.0, v + dv.min(self.height - v) - w / 2.0];
            around(r, openings).into_iter().map(|r| self.panel(f, r[0], r[1], r[2] - r[0], r[3] - r[1])).collect()
        };
        let mut pieces: Vec<Piece> = Vec::new();
        // the joins between members: their kind, the member that runs through (or the first of a
        // mitre) and the one that stops
        let mut joins: Vec<(JoinKind, usize, usize)> = Vec::new();
        let corner = match f.corners {
            Corner::Butt => JoinKind::Butt,
            Corner::Mitre => JoinKind::Mitre,
        };
        let mitred = f.corners == Corner::Mitre;
        // an edge's vertical member reaches the outer faces of the rows' lines at the bottom and the
        // top, unless the corners are mitred, where both members meet on their centre lines
        let span = |edge: bool| if edge && mitred { (0.0, self.height) } else { (-w / 2.0, self.height + w / 2.0) };
        match f.pattern {
            Pattern::Stack => {
                if let Some((size, _)) = f.member {
                    // the vertical members on every division along run through; the horizontal
                    // ones on every division up stop against them, a mitre at the corners
                    let mut verticals = Vec::with_capacity(bays + 1);
                    for i in 0..=bays {
                        let (from, to) = span(i == 0 || i == bays);
                        pieces.push(self.vertical(f, size, i as f64 * bay, from, to)?);
                        verticals.push(pieces.len() - 1);
                    }
                    for i in 0..bays {
                        for j in 0..=rows {
                            pieces.push(self.horizontal(f, size, j as f64 * row, i as f64 * bay, (i + 1) as f64 * bay)?);
                            let h = pieces.len() - 1;
                            let edge_row = j == 0 || j == rows;
                            joins.push((if edge_row && i == 0 { corner } else { JoinKind::Butt }, verticals[i], h));
                            joins.push((if edge_row && i + 1 == bays { corner } else { JoinKind::Butt }, verticals[i + 1], h));
                        }
                    }
                }
                for i in 0..bays {
                    for j in 0..rows {
                        parts.extend(panel(i as f64 * bay, j as f64 * row, bay, row)?);
                    }
                }
            }
            Pattern::RunningBond => {
                // the edges and the rows' lines; the rows' lines stop against the edges, a mitre
                // at the corners
                let mut lines_up: Vec<usize> = Vec::new();
                if let Some((size, _)) = f.member {
                    let (from, to) = span(true);
                    pieces.push(self.vertical(f, size, 0.0, from, to)?);
                    let first = pieces.len() - 1;
                    pieces.push(self.vertical(f, size, self.length, from, to)?);
                    let last = pieces.len() - 1;
                    for j in 0..=rows {
                        pieces.push(self.horizontal(f, size, j as f64 * row, 0.0, self.length)?);
                        let h = pieces.len() - 1;
                        let kind = if j == 0 || j == rows { corner } else { JoinKind::Butt };
                        joins.push((kind, first, h));
                        joins.push((kind, last, h));
                        lines_up.push(h);
                    }
                }
                for j in 0..rows {
                    let v = j as f64 * row;
                    // the divisions of this row: every bay, shifted half a bay in every second
                    // row; one too close to the one before it (or to the end) is left out
                    let shift = if j % 2 == 1 { bay / 2.0 } else { 0.0 };
                    let mut lines = vec![0.0];
                    for k in 0..=bays {
                        let u = k as f64 * bay + shift;
                        if u - lines[lines.len() - 1] > w + LINEAR_TOLERANCE && self.length - u > w + LINEAR_TOLERANCE {
                            lines.push(u);
                        }
                    }
                    lines.push(self.length);
                    if let Some((size, _)) = f.member {
                        // the joints between the units stop against the rows' lines, so they do
                        // not run through
                        for &u in &lines[1..lines.len() - 1] {
                            pieces.push(self.vertical(f, size, u, j as f64 * row, (j + 1) as f64 * row)?);
                            let joint = pieces.len() - 1;
                            joins.push((JoinKind::Butt, lines_up[j], joint));
                            joins.push((JoinKind::Butt, lines_up[j + 1], joint));
                        }
                    }
                    for pair in lines.windows(2) {
                        parts.extend(panel(pair[0], v, pair[1] - pair[0], row)?);
                    }
                }
            }
        }
        // the joins, as a join element's, through the kernel's join geometry
        for (kind, a, b) in joins {
            let done = join::cuts(kind, &[pieces[a].member.clone(), pieces[b].member.clone()])?;
            let piece = |k: usize| if k == 0 { a } else { b };
            for (k, end, plane) in done.trims {
                let p = &mut pieces[piece(k)];
                match end {
                    MemberEnd::Start => p.start = Some(plane),
                    MemberEnd::End => p.end = Some(plane),
                }
            }
            for (k, solid) in done.additions {
                pieces[piece(k)].additions.push(solid);
            }
            for (k, solid) in done.removals {
                pieces[piece(k)].removals.push(solid);
            }
            parts.extend(done.parts);
        }
        let material = f.member.and_then(|(_, m)| m).map(str::to_string);
        for p in pieces {
            let solid = join::finish(&p.member, p.start, p.end, &p.additions, &p.removals)?;
            parts.push(PartSolid { class: "IfcMember", solid, material: material.clone() });
        }
        Ok(())
    }
}

/// Whether an opening is a door: it has no sill.
fn is_door(o: &Opening) -> bool {
    o.sill <= LINEAR_TOLERANCE
}

/// Checks openings against a frame `length` by `height` framed as `framing`, its members `w` wide:
/// each with its king studs inside the end studs, its header under the top plates and, a window,
/// its sill over the bottom plates; their framing apart.
fn framed_openings(length: f64, height: f64, framing: Framing, w: f64, openings: &[Opening]) -> Result<(), Refusal> {
    let tol = LINEAR_TOLERANCE;
    let (bottom, top) = (f64::from(framing.bottom_plates) * w, height - f64::from(framing.top_plates) * w);
    let mut spans = Vec::with_capacity(openings.len());
    for o in openings {
        if !positive(o.width) || !positive(o.height) || !o.along.is_finite() || !o.sill.is_finite() {
            return Err(Refusal::NotPositive);
        }
        let along = o.along - 3.0 * w >= -tol && o.along + o.width + 3.0 * w <= length + tol;
        let foot = if is_door(o) { o.sill >= -tol } else { o.sill - w >= bottom - tol };
        if !along || !foot || o.sill + o.height + framing.header > top + tol {
            return Err(Refusal::OpeningOutside);
        }
        spans.push((o.along - 2.0 * w, o.along + o.width + 2.0 * w));
    }
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    if spans.windows(2).any(|p| p[1].0 < p[0].1 - tol) {
        return Err(Refusal::OpeningsOverlap);
    }
    Ok(())
}

/// Rectangle `r` (`[u0, v0, u1, v1]`) less the openings: for each, the strips beside it, then the
/// pieces below and above it.
fn around(r: [f64; 4], openings: &[Opening]) -> Vec<[f64; 4]> {
    let tol = LINEAR_TOLERANCE;
    let mut rects = vec![r];
    for o in openings {
        let (a, b, s, t) = (o.along, o.along + o.width, o.sill.max(0.0), o.sill + o.height);
        let mut cut = Vec::with_capacity(rects.len() + 3);
        for r in rects {
            if a >= r[2] - tol || b <= r[0] + tol || s >= r[3] - tol || t <= r[1] + tol {
                cut.push(r);
                continue;
            }
            if a > r[0] + tol {
                cut.push([r[0], r[1], a, r[3]]);
            }
            if b < r[2] - tol {
                cut.push([b, r[1], r[2], r[3]]);
            }
            let (u0, u1) = (a.max(r[0]), b.min(r[2]));
            if s > r[1] + tol {
                cut.push([u0, r[1], u1, s]);
            }
            if t < r[3] - tol {
                cut.push([u0, t, u1, r[3]]);
            }
        }
        rects = cut;
    }
    rects
}

/// A framed face being laid out: its members and the cells between them, as rectangles
/// `[u0, v0, u1, v1]` in the face.
struct Frame<'a> {
    line: &'a Line,
    face: &'a Face<'a>,
    size: DVec2,
    members: Vec<Piece>,
    cells: Vec<[f64; 4]>,
}

impl Frame<'_> {
    /// A stud centred at `u`, from `from` up to `to`, if that is a length.
    fn stud(&mut self, u: f64, from: f64, to: f64) -> Result<(), Refusal> {
        if to - from > LINEAR_TOLERANCE {
            self.members.push(self.line.vertical(self.face, self.size, u, from, to)?);
        }
        Ok(())
    }

    /// A horizontal member `high` in the face, centred at `v`, from `from` along to `to`, if that
    /// is a length.
    fn rail(&mut self, v: f64, from: f64, to: f64, high: f64) -> Result<(), Refusal> {
        if to - from > LINEAR_TOLERANCE {
            self.members.push(self.line.horizontal(self.face, DVec2::new(high, self.size.y), v, from, to)?);
        }
        Ok(())
    }

    /// Fills the room between members ending at `lo` and starting at `hi`, from `v0` up to `v1`:
    /// studs at the bay's centres, noggins between them at the rows, and the cells between those.
    fn fill(&mut self, lo: f64, hi: f64, v0: f64, v1: f64) -> Result<(), Refusal> {
        let tol = LINEAR_TOLERANCE;
        if hi - lo <= tol || v1 - v0 <= tol {
            return Ok(());
        }
        let w = self.size.x;
        let studs = inner_lines(lo, hi, self.face.spacing.x, w);
        for &u in &studs {
            self.stud(u, v0, v1)?;
        }
        let noggins = inner_lines(v0, v1, self.face.spacing.y, w);
        let mut edges = vec![lo];
        for &u in &studs {
            edges.extend([u - w / 2.0, u + w / 2.0]);
        }
        edges.push(hi);
        let mut bands = vec![v0];
        for &v in &noggins {
            bands.extend([v - w / 2.0, v + w / 2.0]);
        }
        bands.push(v1);
        // studs that stand against one another have no noggin or cell between them
        for column in edges.chunks(2).filter(|c| c[1] - c[0] > tol) {
            for &v in &noggins {
                self.rail(v, column[0], column[1], w)?;
            }
            for band in bands.chunks(2).filter(|b| b[1] - b[0] > tol) {
                self.cells.push([column[0], band[0], column[1], band[1]]);
            }
        }
        Ok(())
    }
}

/// The centres of the members `w` wide a frame needs between an edge member ending at `lo` and one
/// starting at `hi`: one at every multiple of `pitch` from the face's start (or foot). A line on an
/// edge member needs none; a member that would overlap one stands against it instead, unless there
/// is no room for it.
fn inner_lines(lo: f64, hi: f64, pitch: f64, w: f64) -> Vec<f64> {
    let tol = LINEAR_TOLERANCE;
    let mut lines: Vec<f64> = Vec::new();
    if hi - lo < w {
        return lines;
    }
    let first = (lo / pitch).floor().max(0.0) as usize + 1;
    for k in first.. {
        let at = k as f64 * pitch;
        if at >= hi - tol {
            break;
        }
        if at <= lo + tol {
            continue;
        }
        let at = at.clamp(lo + w / 2.0, hi - w / 2.0);
        if lines.last().is_some_and(|&last| at - last < w - tol) {
            continue;
        }
        lines.push(at);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(length: f64, height: f64) -> SystemDef {
        SystemDef { start: DVec2::ZERO, end: DVec2::new(length, 0.0), base: 0.0, height, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new() }
    }

    fn typed(length: f64, height: f64, id: &str) -> SystemDef {
        system(length, height).typed_from(&Library::starter(), id).unwrap()
    }

    fn of<'a>(parts: &'a [PartSolid], material: &str) -> Vec<&'a PartSolid> {
        parts.iter().filter(|p| p.material.as_deref() == Some(material)).collect()
    }

    #[test]
    fn a_typed_curtain_wall_has_its_type_sizes_and_materials() {
        let s = typed(6.0, 3.6, "system.curtain-wall-aluminium-1500");
        assert_eq!((s.spacing, s.member, s.panel), (DVec2::new(1.5, 1.8), DVec2::new(0.05, 0.15), 0.024));
        let parts = parts(&s).unwrap();
        // 5 mullions, 4 bays of 3 transoms, 8 panes
        assert_eq!((of(&parts, "aluminium").len(), of(&parts, "glass").len()), (5 + 4 * 3, 8));
        assert!(parts.iter().all(|p| p.class == if p.material.as_deref() == Some("glass") { "IfcPlate" } else { "IfcMember" }));
        // an untyped system has no materials
        assert!(super::parts(&system(6.0, 3.0)).unwrap().iter().all(|p| p.material.is_none()));
    }

    #[test]
    fn a_running_bond_shifts_every_second_row_half_a_brick() {
        // 2.2 m by 0.625 m: ten bays of 220 mm and ten courses of 62.5 mm
        let s = typed(2.2, 0.625, "system.masonry-waal-stretcher");
        let parts = parts(&s).unwrap();
        let bricks = of(&parts, "brick");
        // ten bricks in a course on the grid, nine and two halves in a shifted one
        assert_eq!(bricks.len(), 5 * 10 + 5 * 11);
        let (w, row, l) = (0.01, 0.0625, 2.2);
        let volume: f64 = bricks.iter().map(|b| b.solid.volume()).sum();
        let expected = 0.1 * (row - w) * (5.0 * (l - 10.0 * w) + 5.0 * (l - 11.0 * w));
        assert!((volume - expected).abs() < 1e-9, "{volume} vs {expected}");
        // the second course starts with half a brick
        let first = |course: usize| {
            let v = course as f64 * row + w / 2.0;
            let mut row: Vec<_> = bricks.iter().map(|b| b.solid.bounds()).filter(|(lo, _)| (lo.z - v).abs() < 1e-9).collect();
            row.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
            row[0].1.x - row[0].0.x
        };
        assert!((first(0) - (0.22 - w)).abs() < 1e-9 && (first(1) - (0.11 - w)).abs() < 1e-9, "{} {}", first(0), first(1));
        // the mortar: two edges, eleven bed joints, and the head joints of each course
        assert_eq!(of(&parts, "mortar").len(), 2 + 11 + 5 * 9 + 5 * 10);
    }

    /// The bounds of the parts of `material` on a face along +x, as (along, up) rectangles, sorted.
    fn rects(parts: &[PartSolid], material: &str) -> Vec<[f64; 4]> {
        let mut r: Vec<[f64; 4]> = of(parts, material).iter().map(|p| p.solid.bounds()).map(|(lo, hi)| [lo.x, lo.z, hi.x, hi.z]).collect();
        r.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
        r
    }

    /// The frame's own members (as deep as its studs) of a timber frame `depth` deep.
    fn frame(parts: &[PartSolid], depth: f64) -> Vec<[f64; 4]> {
        let deep: Vec<PartSolid> = parts.iter().filter(|p| p.material.as_deref() == Some("timber")).filter(|p| { let (lo, hi) = p.solid.bounds(); (hi.y - lo.y - depth).abs() < 1e-9 }).cloned().collect();
        rects(&deep, "timber")
    }

    fn near(a: [f64; 4], b: [f64; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn a_timber_frame_has_plates_through_and_studs_from_its_start() {
        // 2.5 m by 2.7 m at 600: studs at either end and at 600, 1200, 1800 and 2400, the last
        // against the end stud
        let (w, d) = (0.038, 0.14);
        let parts = parts(&typed(2.5, 2.7, "system.timber-frame-140")).unwrap();
        let members = frame(&parts, d);
        let plates: Vec<_> = members.iter().filter(|r| (r[2] - r[0] - 2.5).abs() < 1e-9).collect();
        assert_eq!(plates.len(), 2, "a bottom and a top plate through the length");
        assert!(near(*plates[0], [0.0, 0.0, 2.5, w]) && near(*plates[1], [0.0, 2.7 - w, 2.5, 2.7]), "{plates:?}");
        let studs: Vec<f64> = members.iter().filter(|r| (r[3] - r[1] - (2.7 - 2.0 * w)).abs() < 1e-9).map(|r| (r[0] + r[2]) / 2.0).collect();
        let expected = [w / 2.0, 0.6, 1.2, 1.8, 2.4 - (2.4 + w / 2.0 - (2.5 - w)).max(0.0), 2.5 - w / 2.0];
        assert_eq!(studs.len(), expected.len(), "{studs:?}");
        assert!(studs.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-9), "{studs:?}");
        // no noggins under 3 m; the mineral wool fills the bays between the studs, the frame's depth
        assert_eq!(members.len(), 2 + 6);
        let wool: Vec<_> = parts.iter().filter(|p| p.material.as_deref() == Some("mineral-wool") && { let (lo, hi) = p.solid.bounds(); (hi.y - lo.y - d).abs() < 1e-9 }).collect();
        assert_eq!(wool.len(), 5);
        let volume: f64 = wool.iter().map(|p| p.solid.volume()).sum();
        assert!((volume - (2.5 - 6.0 * w) * (2.7 - 2.0 * w) * d).abs() < 1e-9, "{volume}");
        // the frame lies inside the face
        let (lo, hi) = members.iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), r| ([lo[0].min(r[0]), lo[1].min(r[1])], [hi[0].max(r[2]), hi[1].max(r[3])]));
        assert!(near([lo[0], lo[1], hi[0], hi[1]], [0.0, 0.0, 2.5, 2.7]));
    }

    #[test]
    fn a_timber_frame_carries_every_seam_of_its_boards() {
        // 5 m by 3.4 m: boards 1200×3000 from the start, the last ones cut; noggins at 3 m
        let (w, d) = (0.038, 0.14);
        let parts = parts(&typed(5.0, 3.4, "system.timber-frame-140")).unwrap();
        let members = frame(&parts, d);
        for material in ["plywood", "gypsum-board"] {
            let boards = rects(&parts, material);
            assert_eq!(boards.len(), 5 * 2, "{material}: five columns of a whole board and a cut one");
            assert!(near(boards[0], [0.0, 0.0, 1.2, 3.0]) && near(boards[1], [0.0, 3.0, 1.2, 3.4]) && near(boards[9], [4.8, 3.0, 5.0, 3.4]), "{boards:?}");
            let area: f64 = boards.iter().map(|b| (b[2] - b[0]) * (b[3] - b[1])).sum();
            assert!((area - 5.0 * 3.4).abs() < 1e-9);
            // every seam inside the face has a member of the frame behind it
            for b in &boards {
                for (u, v) in [(b[0], (b[1] + b[3]) / 2.0), (b[2], (b[1] + b[3]) / 2.0), ((b[0] + b[2]) / 2.0, b[1]), ((b[0] + b[2]) / 2.0, b[3])] {
                    let inside = u > 1e-9 && u < 5.0 - 1e-9 && v > 1e-9 && v < 3.4 - 1e-9;
                    let carried = members.iter().any(|m| m[0] - 1e-9 <= u && u <= m[2] + 1e-9 && m[1] - 1e-9 <= v && v <= m[3] + 1e-9);
                    assert!(!inside || carried, "{material}: seam at {u} {v} without timber behind it");
                }
            }
        }
        // a noggin row at 3 m between every pair of studs; the battens of the service zone too
        let noggins: Vec<_> = members.iter().filter(|r| (r[1] - (3.0 - w / 2.0)).abs() < 1e-9).collect();
        assert_eq!(noggins.len(), rects(&parts, "timber").iter().filter(|r| (r[1] - (3.0 - w / 2.0)).abs() < 1e-9).count());
        assert_eq!(noggins.len(), 9, "{noggins:?}");
        let battens = rects(&parts, "timber").into_iter().filter(|r| (r[1] - (3.0 - 0.0225)).abs() < 1e-9).count();
        assert_eq!(battens, 9);
        // the membranes in courses of 1.5 m, the last cut
        assert_eq!(rects(&parts, "breather-membrane").iter().map(|r| r[3] - r[1]).map(|h| (h * 1e6).round() / 1e6).collect::<Vec<_>>(), [1.5, 1.5, 0.4]);
    }

    #[test]
    fn a_timber_frame_has_its_layers_through_its_depth() {
        // along +x the exterior is to the right, at -y: the breather membrane there, then the
        // frame, plywood, the vapour control layer, the service zone and gypsum board inside
        let parts = parts(&typed(2.4, 2.7, "system.timber-frame-140")).unwrap();
        let across = |material: &str| {
            of(&parts, material).iter().map(|p| p.solid.bounds()).fold((f64::MAX, f64::MIN), |(lo, hi), (a, b)| (lo.min(a.y), hi.max(b.y)))
        };
        let near = |(a, b): (f64, f64), (c, d): (f64, f64)| (a - c).abs() < 1e-9 && (b - d).abs() < 1e-9;
        assert!(near(across("breather-membrane"), (-0.0705, -0.07)), "{:?}", across("breather-membrane"));
        assert!(near(across("plywood"), (0.07, 0.088)), "{:?}", across("plywood"));
        assert!(near(across("vapour-control-membrane"), (0.088, 0.0882)));
        assert!(near(across("gypsum-board"), (0.1332, 0.1457)), "{:?}", across("gypsum-board"));
        assert!(near(across("timber"), (-0.07, 0.1332)), "the studs and the service battens");
        // the boards of 1200 edge to edge; the battens framed as the studs: bottom and top, and at
        // the studs' centres
        assert_eq!((of(&parts, "plywood").len(), of(&parts, "gypsum-board").len()), (2, 2));
        assert_eq!(of(&parts, "timber").len(), 2 * (2 + 5));
        // an interior frame has gypsum board on either face and nothing else
        let inner = super::parts(&typed(2.4, 2.7, "system.timber-frame-internal-89")).unwrap();
        let materials: std::collections::BTreeSet<_> = inner.iter().filter_map(|p| p.material.as_deref()).collect();
        assert_eq!(materials.into_iter().collect::<Vec<_>>(), ["gypsum-board", "mineral-wool", "timber"]);
    }

    /// Timber frame `id`, `length` by `height`, with `openings`.
    fn opened(length: f64, height: f64, id: &str, openings: Vec<Opening>) -> SystemDef {
        SystemDef { openings, ..typed(length, height, id) }
    }

    /// Whether every seam of the boards of `material`, inside the face and off the openings' edges,
    /// has a member of the frame behind it.
    fn seams_carried(parts: &[PartSolid], material: &str, members: &[[f64; 4]], size: [f64; 2], openings: &[Opening]) -> Result<(), String> {
        let on_opening = |u: f64, v: f64| {
            openings.iter().any(|o| {
                let (a, b, s, t) = (o.along, o.along + o.width, o.sill, o.sill + o.height);
                let within = |x: f64, lo: f64, hi: f64| x >= lo - 1e-9 && x <= hi + 1e-9;
                ((u - a).abs() < 1e-9 || (u - b).abs() < 1e-9) && within(v, s, t) || ((v - s).abs() < 1e-9 || (v - t).abs() < 1e-9) && within(u, a, b)
            })
        };
        for b in rects(parts, material) {
            for (u, v) in [(b[0], (b[1] + b[3]) / 2.0), (b[2], (b[1] + b[3]) / 2.0), ((b[0] + b[2]) / 2.0, b[1]), ((b[0] + b[2]) / 2.0, b[3])] {
                let inside = u > 1e-9 && u < size[0] - 1e-9 && v > 1e-9 && v < size[1] - 1e-9;
                let carried = members.iter().any(|m| m[0] - 1e-9 <= u && u <= m[2] + 1e-9 && m[1] - 1e-9 <= v && v <= m[3] + 1e-9);
                if inside && !on_opening(u, v) && !carried {
                    return Err(format!("{material}: seam at {u} {v} without timber behind it"));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn a_window_in_a_timber_frame_is_framed() {
        // 3.6 m by 2.7 m, a window 1.2 by 1.2 from 1.2 along at a sill of 0.9
        let (w, d, header) = (0.038, 0.14, 0.2);
        let window = Opening { along: 1.2, sill: 0.9, width: 1.2, height: 1.2 };
        let parts = parts(&opened(3.6, 2.7, "system.timber-frame-140", vec![window])).unwrap();
        let members = frame(&parts, d);
        let has = |r: [f64; 4]| members.iter().any(|m| near(*m, r));
        // king studs plate to plate, trimmers up to the header, which lies on them
        assert!(has([1.2 - 2.0 * w, w, 1.2 - w, 2.7 - w]) && has([2.4 + w, w, 2.4 + 2.0 * w, 2.7 - w]), "king studs");
        assert!(has([1.2 - w, w, 1.2, 2.1]) && has([2.4, w, 2.4 + w, 2.1]), "trimmers");
        assert!(has([1.2 - w, 2.1, 2.4 + w, 2.1 + header]), "the header over the trimmers");
        assert!(has([1.2, 0.9 - w, 2.4, 0.9]), "the sill between the trimmers");
        // cripples over the trimmers and at the bay's centres between them, above the header, and
        // at 1800 below the sill (the trimmers carry 1200 and 2400 there)
        assert!(has([1.2 - w, 2.1 + header, 1.2, 2.7 - w]) && has([2.4, 2.1 + header, 2.4 + w, 2.7 - w]), "over the trimmers");
        assert!(has([1.8 - w / 2.0, 2.1 + header, 1.8 + w / 2.0, 2.7 - w]), "between them");
        assert!(has([1.8 - w / 2.0, w, 1.8 + w / 2.0, 0.9 - w]), "below the sill");
        assert_eq!(members.len(), 2 + 2 + 4 + 2 + 2 + 3 + 1, "plates, end studs, kings and trimmers, header and sill, studs at 600 and 3000, cripples");
        // the frame, its cells and the opening fill the face: nothing overlaps, nothing is missing
        let wool: Vec<[f64; 4]> = parts.iter().filter(|p| p.material.as_deref() == Some("mineral-wool") && { let (lo, hi) = p.solid.bounds(); (hi.y - lo.y - d).abs() < 1e-9 }).map(|p| { let (lo, hi) = p.solid.bounds(); [lo.x, lo.z, hi.x, hi.z] }).collect();
        let area = |rs: &[[f64; 4]]| rs.iter().map(|r| (r[2] - r[0]) * (r[3] - r[1])).sum::<f64>();
        assert!((area(&members) + area(&wool) + 1.2 * 1.2 - 3.6 * 2.7).abs() < 1e-9, "{} {}", area(&members), area(&wool));
        // the boards are cut around the window and every seam has timber behind it
        for material in ["plywood", "gypsum-board", "breather-membrane"] {
            assert!((area(&rects(&parts, material)) - (3.6 * 2.7 - 1.2 * 1.2)).abs() < 1e-9, "{material}");
            assert!(rects(&parts, material).iter().all(|r| r[2] <= 1.2 + 1e-9 || r[0] >= 2.4 - 1e-9 || r[3] <= 0.9 + 1e-9 || r[1] >= 2.1 - 1e-9), "{material} in the window");
        }
        seams_carried(&parts, "plywood", &members, [3.6, 2.7], &[window]).unwrap();
        assert_eq!(rects(&parts, "plywood").len(), 4, "a whole board either side, one below and one above the window");
    }

    #[test]
    fn a_door_stops_the_bottom_plate_and_has_no_sill() {
        let (w, d) = (0.038, 0.14);
        let door = Opening { along: 0.6, sill: 0.0, width: 0.9, height: 2.1 };
        let parts = parts(&opened(3.0, 2.7, "system.timber-frame-140", vec![door])).unwrap();
        let members = frame(&parts, d);
        let bottom: Vec<_> = members.iter().filter(|r| r[1].abs() < 1e-9).collect();
        assert_eq!(bottom.len(), 2, "{bottom:?}");
        assert!(near(*bottom[0], [0.0, 0.0, 0.6, w]) && near(*bottom[1], [1.5, 0.0, 3.0, w]));
        // nothing of any layer in the door
        for p in &parts {
            let (lo, hi) = p.solid.bounds();
            let apart = hi.x <= 0.6 + 1e-9 || lo.x >= 1.5 - 1e-9 || lo.z >= 2.1 - 1e-9;
            assert!(apart, "{:?} {lo} {hi}", p.material);
        }
        seams_carried(&parts, "gypsum-board", &members, [3.0, 2.7], &[door]).unwrap();
    }

    #[test]
    fn openings_that_do_not_fit_a_frame_are_refused() {
        let frame = |openings: Vec<Opening>| parts(&opened(3.6, 2.7, "system.timber-frame-140", openings)).map(|p| p.len());
        let window = |along: f64, sill: f64, width: f64, height: f64| Opening { along, sill, width, height };
        assert_eq!(frame(vec![window(0.05, 0.9, 1.0, 1.2)]), Err(Refusal::OpeningOutside), "no room for its king stud");
        assert_eq!(frame(vec![window(1.0, 0.9, 1.0, 1.7)]), Err(Refusal::OpeningOutside), "its header through the top plate");
        assert_eq!(frame(vec![window(1.0, 0.03, 1.0, 1.2)]), Err(Refusal::OpeningOutside), "its sill in the bottom plate");
        assert_eq!(frame(vec![window(0.5, 0.9, 1.0, 1.2), window(1.6, 0.9, 1.0, 1.2)]), Err(Refusal::OpeningsOverlap), "their king studs in one another");
        assert_eq!(frame(vec![window(1.0, 0.9, 0.0, 1.2)]), Err(Refusal::NotPositive));
        // two openings with their king studs side by side are framed; the service zone frames them
        // too, with its battens 45 wide, so they are as far apart as its framing needs
        assert!(frame(vec![window(0.5, 0.9, 1.0, 1.2), window(1.5 + 4.0 * 0.045, 0.9, 1.0, 1.2)]).is_ok());
        // a curtain wall (not framed) takes none
        let curtain = SystemDef { openings: vec![window(1.0, 0.9, 1.0, 1.2)], ..system(3.6, 2.7) };
        assert_eq!(parts(&curtain).map(|p| p.len()), Err(Refusal::OpeningNotFramed));
    }

    #[test]
    fn a_board_is_cut_around_an_opening_into_rectangles() {
        let o = Opening { along: 1.0, sill: 0.5, width: 1.0, height: 1.0 };
        // beside it full height, then below and above it
        assert_eq!(around([0.0, 0.0, 3.0, 3.0], &[o]), vec![[0.0, 0.0, 1.0, 3.0], [2.0, 0.0, 3.0, 3.0], [1.0, 0.0, 2.0, 0.5], [1.0, 1.5, 2.0, 3.0]]);
        assert_eq!(around([2.0, 0.0, 3.0, 3.0], &[o]), vec![[2.0, 0.0, 3.0, 3.0]], "a board beside it is whole");
        assert!(around([1.0, 0.5, 2.0, 1.5], &[o]).is_empty(), "a board in it is gone");
    }

    #[test]
    fn a_frame_too_small_for_its_studs_is_refused() {
        assert_eq!(parts(&typed(0.07, 2.7, "system.timber-frame-140")).map(|p| p.len()), Err(Refusal::NotPositive), "no room for two end studs");
        assert_eq!(parts(&typed(2.4, 0.07, "system.timber-frame-140")).map(|p| p.len()), Err(Refusal::NotPositive), "no room between the plates");
        // a frame just wider than its end studs is two studs and two plates
        let narrow = parts(&typed(0.1, 2.7, "system.timber-frame-internal-89")).unwrap();
        assert_eq!(of(&narrow, "timber").len(), 4);
    }

    #[test]
    fn a_line_on_an_edge_member_needs_none_and_one_that_would_overlap_it_stands_against_it() {
        let w = 0.038;
        let lines = inner_lines(w, 2.4 - w, 0.6, w);
        assert!(lines.len() == 3 && lines.iter().zip([0.6, 1.2, 1.8]).all(|(a, b)| (a - b).abs() < 1e-12), "{lines:?}");
        // a stud at 2.4 would overlap the end stud of a 2.45 frame: it stands against it
        let lines = inner_lines(w, 2.45 - w, 0.6, w);
        assert!(lines.len() == 4 && (lines[3] - (2.45 - 1.5 * w)).abs() < 1e-12, "{lines:?}");
        // 2.4 is on the end stud of a 2.42 frame: none
        assert_eq!(inner_lines(w, 2.42 - w, 0.6, w).len(), 3);
        assert!(inner_lines(w, w + 0.01, 0.6, w).is_empty());
    }

    /// The volume of a ring of members `w` wide and `depth` deep on the edges of a face `length`
    /// by `height`, centred on them.
    fn ring(length: f64, height: f64, w: f64, depth: f64) -> f64 {
        ((length + w) * (height + w) - (length - w) * (height - w)) * depth
    }

    #[test]
    fn a_frame_is_closed_at_its_corners_butted_or_mitred() {
        // one bay and one row each: four members around one pane
        for (id, length, height, w, depth) in [("system.frame-timber-67x114", 0.9, 1.2, 0.067, 0.114), ("system.frame-aluminium-75", 1.0, 1.4, 0.05, 0.075)] {
            let s = typed(length, height, id);
            let parts = parts(&s).unwrap();
            let members: Vec<_> = parts.iter().filter(|p| p.class == "IfcMember").collect();
            assert_eq!(members.len(), 4, "{id}");
            let volume: f64 = members.iter().map(|p| p.solid.volume()).sum();
            assert!((volume - ring(length, height, w, depth)).abs() < 1e-9, "{id}: {volume} vs {}", ring(length, height, w, depth));
            // the left member reaches the outer corner, below the frame's line
            let left = members.iter().map(|p| p.solid.bounds()).find(|(_, hi)| hi.x < w && hi.z > height / 2.0).unwrap();
            assert!((left.0.z + w / 2.0).abs() < 1e-9, "{id}: {left:?}");
        }
        // the aluminium frame's left member is cut on the diagonal at both ends: as long as the
        // frame is high on its centre line, where the butted timber one reaches past both rails
        let left = |id: &str, length: f64, height: f64, w: f64| {
            let parts = parts(&typed(length, height, id)).unwrap();
            parts.iter().filter(|p| p.class == "IfcMember").find(|p| p.solid.bounds().1.x < w && p.solid.bounds().1.z > height / 2.0).unwrap().solid.volume()
        };
        assert!((left("system.frame-aluminium-75", 1.0, 1.4, 0.05) - 1.4 * 0.05 * 0.075).abs() < 1e-9);
        assert!((left("system.frame-timber-67x114", 0.9, 1.2, 0.067) - (1.2 + 0.067) * 0.067 * 0.114).abs() < 1e-9);
    }

    #[test]
    fn a_curtain_wall_is_closed_where_its_members_meet() {
        // two bays and one row: the rows' lines stop against the mullions, which reach their faces
        let s = SystemDef { spacing: DVec2::new(1.5, 3.0), ..system(3.0, 3.0) };
        let parts = parts(&s).unwrap();
        let (w, depth) = (0.05, 0.15);
        let volume: f64 = parts.iter().filter(|p| p.class == "IfcMember").map(|p| p.solid.volume()).sum();
        let middle = (3.0 - w) * w * depth;
        assert!((volume - (ring(3.0, 3.0, w, depth) + middle)).abs() < 1e-9, "{volume}");
    }

    #[test]
    fn a_system_too_big_to_generate_is_refused() {
        // a hundred metres of brickwork ten metres high: over 70 000 bricks
        assert_eq!(parts(&typed(100.0, 10.0, "system.masonry-waal-stretcher")).map(|p| p.len()), Err(Refusal::TooManyParts));
        let mut s = typed(3.0, 3.0, "system.timber-frame-140");
        s.system_type.as_mut().unwrap().layers[1].grid.bay = 0.0;
        assert_eq!(parts(&s).map(|p| p.len()), Err(Refusal::NotPositive), "a layer without bays");
    }

    #[test]
    fn a_lying_type_does_not_make_an_upright_system() {
        assert_eq!(system(3.0, 3.0).typed_from(&Library::starter(), "system.ceiling-600x600"), None);
        assert_eq!(system(3.0, 3.0).typed_from(&Library::starter(), "system.unknown"), None);
    }

    #[test]
    fn a_typed_system_keeps_its_type_in_json_and_an_old_one_reads() {
        let s = opened(3.0, 3.0, "system.timber-frame-184", vec![Opening { along: 1.0, sill: 0.0, width: 0.9, height: 2.1 }]);
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<SystemDef>(&text).unwrap(), s);
        assert!(text.contains("\"openings\"") && !serde_json::to_string(&typed(3.0, 3.0, "system.timber-frame-184")).unwrap().contains("openings"));
        let old = r#"{"start":[0.0,0.0],"end":[3.0,0.0],"base":0.0,"height":3.0,"spacing":[1.5,1.5],"member":[0.05,0.15],"panel":0.02}"#;
        assert_eq!(serde_json::from_str::<SystemDef>(old).unwrap(), system(3.0, 3.0));
        assert!(!serde_json::to_string(&system(3.0, 3.0)).unwrap().contains("system_type"));
    }
}
