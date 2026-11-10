//! The system generator: the members and panels of a system, and the type a system is made by.
//!
//! A system is a vertical face on its line ([`SystemDef`]), divided into equal bays along it and
//! rows up it. Its type, copied from a library as a build-up is, adds what a curtain wall alone
//! lacks: the pattern of its rows (a running bond shifts every second row half a bay), the material
//! of its members and of its panels, and layers across the face, each with a grid of its own (the
//! boards on a timber frame). Without a type a system is a plain curtain wall.
//!
//! Members are centred on the grid's lines, the edges included, and their ends are square. In a
//! stacked grid the vertical members run through and the horizontal ones stop against them; in a
//! running bond the horizontal ones run through between the edges and the vertical ones stop
//! against them, so a bond's joints do not run through. Panels fill the bays between the members,
//! centred on their face; a layer without members lays its panels edge to edge.

use crate::element::{PartSolid, SystemDef};
use crate::profile::Profile;
use crate::solid::{Axes, Solid};
use crate::{positive, Refusal, LINEAR_TOLERANCE};
use bim_library::system::{Pattern, SystemLayer, SystemType};
use bim_library::Library;
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

/// The most parts one system is generated as; a system that would have more is refused.
pub const MAX_PARTS: usize = 50_000;

/// A system's type as the system keeps it: copied from the library, so the element is whole
/// without it. The sizes of the system's own face are its `spacing`, `member` and `panel`; this
/// holds the type's id, the pattern of the face's rows, the material of its members and its
/// panels, and the layers across it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemTypeDef {
    pub id: String,
    #[serde(default, skip_serializing_if = "is_stack")]
    pub pattern: Pattern,
    pub member_material: String,
    pub panel_material: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<SystemLayer>,
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
        });
        Some(self)
    }

    /// This system made by type `id` of `library`; `None` when the library lacks it or it lies.
    pub fn typed_from(self, library: &Library, id: &str) -> Option<Self> {
        self.typed(library.system_type(id)?)
    }
}

/// One face of a system: its offset across the system's face (to the right of its line, its
/// exterior, positive), its grid, its members (size in the face and across it, material) and its
/// panels (thickness, material).
struct Face<'a> {
    offset: f64,
    spacing: DVec2,
    pattern: Pattern,
    member: Option<(DVec2, Option<&'a str>)>,
    panel: (f64, Option<&'a str>),
}

impl Face<'_> {
    fn width(&self) -> f64 {
        self.member.map_or(0.0, |(m, _)| m.x)
    }

    /// The bays and rows on a face `length` long and `height` high, and their size.
    fn division(&self, length: f64, height: f64) -> (usize, usize, f64, f64) {
        // a count beyond any system's (and beyond what usize holds) is refused by the part count
        let count = |extent: f64, pitch: f64| ((extent / pitch).round().min(1e12) as usize).max(1);
        let (bays, rows) = (count(length, self.spacing.x), count(height, self.spacing.y));
        (bays, rows, length / bays as f64, height / rows as f64)
    }

    /// At most how many parts it is generated as.
    fn most_parts(&self, length: f64, height: f64) -> usize {
        let (bays, rows, _, _) = self.division(length, height);
        let cells = bays.saturating_mul(rows);
        match self.pattern {
            // the vertical members, the horizontal ones between them, the panels
            Pattern::Stack => (bays + 1).saturating_add(cells.saturating_add(bays)).saturating_add(cells),
            // the edges and the rows' lines, then per row one division and one panel more than
            // its bays at most
            Pattern::RunningBond => (rows + 3).saturating_add(rows.saturating_mul(bays + 1).saturating_mul(2)),
        }
    }

    /// Checks it: positive sizes, bays and rows wider than the members.
    fn check(&self, length: f64, height: f64) -> Result<(), Refusal> {
        let sizes = [self.spacing.x, self.spacing.y, self.panel.0];
        let member = self.member.map_or(Vec::new(), |(m, _)| vec![m.x, m.y]);
        if !sizes.into_iter().chain(member).all(positive) || !self.offset.is_finite() {
            return Err(Refusal::NotPositive);
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
        member: Some((s.member, t.map(|t| t.member_material.as_str()))),
        panel: (s.panel, t.map(|t| t.panel_material.as_str())),
    };
    let layers = t.into_iter().flat_map(|t| &t.layers).map(|l| Face {
        offset: l.offset,
        spacing: DVec2::new(l.grid.bay, l.grid.row),
        pattern: l.grid.pattern,
        member: l.members.as_ref().map(|m| (DVec2::new(m.width, m.depth), Some(m.material.as_str()))),
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
    for f in &faces {
        f.check(length, s.height)?;
    }
    if faces.iter().map(|f| f.most_parts(length, s.height)).fold(0usize, usize::saturating_add) > MAX_PARTS {
        return Err(Refusal::TooManyParts);
    }
    let dir = (d / length).extend(0.0);
    let line = Line { origin: s.start.extend(s.base), dir, right: dir.cross(DVec3::Z), length, height: s.height };
    let mut parts = Vec::new();
    for f in &faces {
        line.generate(f, &mut parts)?;
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

impl Line {
    fn generate(&self, f: &Face, parts: &mut Vec<PartSolid>) -> Result<(), Refusal> {
        let (bays, rows, bay, row) = f.division(self.length, self.height);
        let w = f.width();
        let at = |u: f64, v: f64| self.origin + self.dir * u + DVec3::Z * v + self.right * f.offset;
        let member = |solid| PartSolid { class: "IfcMember", solid, material: f.member.and_then(|(_, m)| m).map(str::to_string) };
        let (thickness, panel_material) = f.panel;
        // a panel `du` by `dv` from (u, v), centred on the face
        let panel = |u: f64, v: f64, du: f64, dv: f64| -> Result<PartSolid, Refusal> {
            let axes = Axes::new(at(u, v) - self.right * (thickness / 2.0), self.dir, self.right)?;
            let p = DVec2::new;
            let face = Profile::new(vec![p(0.0, 0.0), p(du, 0.0), p(du, dv), p(0.0, dv)], Vec::new())?;
            Ok(PartSolid { class: "IfcPlate", solid: Solid::extrude(&face, &axes, thickness)?, material: panel_material.map(str::to_string) })
        };
        // a vertical member at `u` from `v` up `rise`, and a horizontal one at `v` from `u` along `run`
        let vertical = |u: f64, v: f64, rise: f64, (size, _): (DVec2, Option<&str>)| -> Result<Solid, Refusal> {
            Solid::extrude(&Profile::rectangle(size.x, size.y)?, &Axes::new(at(u, v), self.dir, DVec3::Z)?, rise)
        };
        let horizontal = |u: f64, v: f64, run: f64, (size, _): (DVec2, Option<&str>)| -> Result<Solid, Refusal> {
            Solid::extrude(&Profile::rectangle(size.y, size.x)?, &Axes::new(at(u, v), self.right, self.dir)?, run)
        };
        match f.pattern {
            Pattern::Stack => {
                if let Some(m) = f.member {
                    // the vertical members on every division along, the whole height
                    for i in 0..=bays {
                        parts.push(member(vertical(i as f64 * bay, 0.0, self.height, m)?));
                    }
                    // the horizontal ones on every division up, between them
                    for i in 0..bays {
                        for j in 0..=rows {
                            parts.push(member(horizontal(i as f64 * bay + w / 2.0, j as f64 * row, bay - w, m)?));
                        }
                    }
                }
                for i in 0..bays {
                    for j in 0..rows {
                        parts.push(panel(i as f64 * bay + w / 2.0, j as f64 * row + w / 2.0, bay - w, row - w)?);
                    }
                }
            }
            Pattern::RunningBond => {
                if let Some(m) = f.member {
                    // the edges the whole height, the rows' lines through between them
                    for u in [0.0, self.length] {
                        parts.push(member(vertical(u, 0.0, self.height, m)?));
                    }
                    for j in 0..=rows {
                        parts.push(member(horizontal(w / 2.0, j as f64 * row, self.length - w, m)?));
                    }
                }
                for j in 0..rows {
                    let v = j as f64 * row + w / 2.0;
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
                    if let Some(m) = f.member {
                        for &u in &lines[1..lines.len() - 1] {
                            parts.push(member(vertical(u, v, row - w, m)?));
                        }
                    }
                    for pair in lines.windows(2) {
                        parts.push(panel(pair[0] + w / 2.0, v, pair[1] - pair[0] - w, row - w)?);
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(length: f64, height: f64) -> SystemDef {
        SystemDef { start: DVec2::ZERO, end: DVec2::new(length, 0.0), base: 0.0, height, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None }
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

    #[test]
    fn a_timber_frame_has_its_boards_on_either_face() {
        // 2.4 m by 2.7 m: four bays of studs at 600, one row
        let parts = parts(&typed(2.4, 2.7, "system.timber-frame-140")).unwrap();
        assert_eq!(of(&parts, "timber").len(), 5 + 4 * 2, "studs, and the plates between them");
        assert_eq!(of(&parts, "mineral-wool").len(), 4);
        let across = |material: &str| {
            of(&parts, material).iter().map(|p| p.solid.bounds()).fold((f64::MAX, f64::MIN), |(lo, hi), (a, b)| (lo.min(a.y), hi.max(b.y)))
        };
        // along +x the exterior is to the right, at -y: OSB there, gypsum board inside
        let near = |(a, b): (f64, f64), (c, d): (f64, f64)| (a - c).abs() < 1e-9 && (b - d).abs() < 1e-9;
        assert!(near(across("osb"), (-0.088, -0.07)), "{:?}", across("osb"));
        assert!(near(across("gypsum-board"), (0.07, 0.0825)), "{:?}", across("gypsum-board"));
        assert_eq!((of(&parts, "osb").len(), of(&parts, "gypsum-board").len()), (2, 2), "boards of 1200, edge to edge");
    }

    #[test]
    fn a_system_too_big_to_generate_is_refused() {
        // a hundred metres of brickwork ten metres high: over 70 000 bricks
        assert_eq!(parts(&typed(100.0, 10.0, "system.masonry-waal-stretcher")).map(|p| p.len()), Err(Refusal::TooManyParts));
        let mut s = typed(3.0, 3.0, "system.timber-frame-140");
        s.system_type.as_mut().unwrap().layers[0].grid.bay = 0.0;
        assert_eq!(parts(&s).map(|p| p.len()), Err(Refusal::NotPositive), "a layer without bays");
    }

    #[test]
    fn a_lying_type_does_not_make_an_upright_system() {
        assert_eq!(system(3.0, 3.0).typed_from(&Library::starter(), "system.ceiling-600x600"), None);
        assert_eq!(system(3.0, 3.0).typed_from(&Library::starter(), "system.unknown"), None);
    }

    #[test]
    fn a_typed_system_keeps_its_type_in_json_and_an_old_one_reads() {
        let s = typed(3.0, 3.0, "system.timber-frame-184");
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<SystemDef>(&text).unwrap(), s);
        let old = r#"{"start":[0.0,0.0],"end":[3.0,0.0],"base":0.0,"height":3.0,"spacing":[1.5,1.5],"member":[0.05,0.15],"panel":0.02}"#;
        assert_eq!(serde_json::from_str::<SystemDef>(old).unwrap(), system(3.0, 3.0));
        assert!(!serde_json::to_string(&system(3.0, 3.0)).unwrap().contains("system_type"));
    }
}
