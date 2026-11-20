//! Wall-local reinforcement regions. Coordinates are metres along the wall and above its base.
use crate::ElementId;
use glam::DVec2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BarRun {
    pub pitch: f64,
    pub first: f64,
    pub depth: f64,
    pub diameter: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Layout {
    Net {
        vertical: BarRun,
        horizontal: BarRun,
    },
    Hairpins {
        pitch: DVec2,
        diameter: f64,
        leg: f64,
        near: f64,
        far: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Area {
    pub name: String,
    pub contour: Vec<DVec2>,
    pub members: Vec<ElementId>,
    pub layout: Layout,
}

/// Interior intervals on a grid line. Half-open edge crossings count polygon vertices once.
/// At the upper boundary sample just inside so the outermost mesh bars are retained.
pub fn spans(contour: &[DVec2], vertical: bool, station: f64) -> Vec<(f64, f64)> {
    let xy = |p: DVec2| if vertical { p } else { DVec2::new(p.y, p.x) };
    let max = contour
        .iter()
        .map(|p| xy(*p).x)
        .fold(f64::NEG_INFINITY, f64::max);
    let x = if (station - max).abs() < 1e-8 {
        max - 1e-9
    } else {
        station
    };
    let mut crossings = Vec::new();
    for (&a, &b) in contour.iter().zip(contour.iter().cycle().skip(1)) {
        let (a, b) = (xy(a), xy(b));
        if (a.x <= x && x < b.x) || (b.x <= x && x < a.x) {
            crossings.push(a.y + (x - a.x) / (b.x - a.x) * (b.y - a.y));
        }
    }
    crossings.sort_by(f64::total_cmp);
    crossings
        .chunks_exact(2)
        .filter(|p| p[1] - p[0] > 1e-6)
        .map(|p| (p[0], p[1]))
        .collect()
}

/// One continuous round bar. The straight definition locates the two tips of the U.
pub fn hairpin_solid(line: &crate::LinearDef, leg: f64) -> Result<crate::Solid, crate::Refusal> {
    use crate::{solid::Face, ProfileDef, Refusal, Solid};
    use glam::DVec3;
    let ProfileDef::Circle { diameter } = line.profile else {
        return Err(Refusal::NotPositive);
    };
    let span = line.end - line.start;
    let radius = span.length() / 2.;
    if !leg.is_finite()
        || leg <= 0.
        || !diameter.is_finite()
        || diameter <= 0.
        || radius <= diameter / 2.
        || span.z.abs() > 1e-6
    {
        return Err(Refusal::NotPositive);
    }
    let across = span.normalize();
    let side = across.cross(DVec3::Z);
    let middle = (line.start + line.end) / 2. - leg * DVec3::Z;
    let turns = crate::profile::circle_segments(radius).div_ceil(2);
    let profile = crate::Profile::circle(diameter)?;
    let n = profile.outer().len() as u32;
    let mut positions = Vec::new();
    let mut ring = |p: DVec3, tangent: DVec3| {
        let other = tangent.cross(side);
        positions.extend(profile.outer().iter().map(|q| p + side * q.x + other * q.y));
    };
    ring(line.start, -DVec3::Z);
    for i in 0..=turns {
        let (s, c) = (std::f64::consts::PI * i as f64 / turns as f64).sin_cos();
        ring(
            middle - radius * (c * across + s * DVec3::Z),
            s * across - c * DVec3::Z,
        );
    }
    ring(line.end, DVec3::Z);
    let last = positions.len() as u32 - n;
    let mut faces = vec![
        Face {
            rings: vec![(0..n).rev().collect()],
        },
        Face {
            rings: vec![(last..last + n).collect()],
        },
    ];
    for ring in (0..last).step_by(n as usize) {
        for i in 0..n {
            let a = ring + i;
            let b = ring + (i + 1) % n;
            faces.push(Face {
                rings: vec![vec![a, b, b + n, a + n]],
            });
        }
    }
    let solid = Solid { positions, faces };
    solid.check()?;
    Ok(solid)
}
