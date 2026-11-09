//! Profiles: closed 2D outlines with holes, the cross-sections of linear elements and the outlines
//! of planar ones. A profile is checked when it is made: simple (no two edges meet but neighbours
//! at their shared corner), not degenerate, holes inside the outline and not in each other. Its
//! outline runs counter-clockwise and its holes clockwise.

use crate::predicates::{orient2d, point_in_ring, ring_orientation, segments_meet, Inside};
use crate::{positive, Refusal};
use glam::DVec2;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// A checked profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    outer: Vec<DVec2>,
    holes: Vec<Vec<DVec2>>,
}

/// Round profiles are polygons whose chords lie within this of the circle (metres).
pub const CHORD_TOLERANCE: f64 = 1e-4;

impl Profile {
    /// Checks and orients an outline with holes. Repeated points (a closing point equal to the
    /// first, or two in a row) and points on a straight line between their neighbours are dropped.
    pub fn new(outer: Vec<DVec2>, holes: Vec<Vec<DVec2>>) -> Result<Self, Refusal> {
        let mut outer = clean(outer)?;
        let mut holes: Vec<Vec<DVec2>> = holes.into_iter().map(clean).collect::<Result<_, _>>()?;
        // simple: no two edges of any rings meet, but neighbours at their shared corner
        let rings: Vec<&Vec<DVec2>> = std::iter::once(&outer).chain(holes.iter()).collect();
        let edges: Vec<(usize, usize, DVec2, DVec2)> = rings
            .iter()
            .enumerate()
            .flat_map(|(r, ring)| (0..ring.len()).map(move |i| (r, i, ring[i], ring[(i + 1) % ring.len()])))
            .collect();
        for (k, &(r1, i1, a, b)) in edges.iter().enumerate() {
            for &(r2, i2, c, d) in &edges[k + 1..] {
                if r1 == r2 {
                    let n = rings[r1].len();
                    if i2 == (i1 + 1) % n || i1 == (i2 + 1) % n {
                        // neighbours: they share one corner; collinear overlap was removed by clean
                        continue;
                    }
                }
                if segments_meet(a, b, c, d) {
                    return Err(Refusal::SelfIntersecting);
                }
            }
        }
        // holes inside the outline and not in each other (no edges meet, so one point decides)
        for (h, hole) in holes.iter().enumerate() {
            if point_in_ring(hole[0], &outer) != Inside::Inside {
                return Err(Refusal::HoleOutside);
            }
            if holes.iter().enumerate().any(|(o, other)| o != h && point_in_ring(hole[0], other) == Inside::Inside) {
                return Err(Refusal::HoleOutside);
            }
        }
        if ring_orientation(&outer) == Ordering::Less {
            outer.reverse();
        }
        for hole in &mut holes {
            if ring_orientation(hole) == Ordering::Greater {
                hole.reverse();
            }
        }
        Ok(Self { outer, holes })
    }

    /// A `width` × `height` rectangle centred on the origin.
    pub fn rectangle(width: f64, height: f64) -> Result<Self, Refusal> {
        if !positive(width) || !positive(height) {
            return Err(Refusal::NotPositive);
        }
        let (w, h) = (width / 2.0, height / 2.0);
        Self::new(vec![DVec2::new(-w, -h), DVec2::new(w, -h), DVec2::new(w, h), DVec2::new(-w, h)], Vec::new())
    }

    /// A circle of `diameter` centred on the origin, as a polygon within [`CHORD_TOLERANCE`].
    pub fn circle(diameter: f64) -> Result<Self, Refusal> {
        if !positive(diameter) {
            return Err(Refusal::NotPositive);
        }
        let r = diameter / 2.0;
        let n = circle_segments(r);
        let points = (0..n).map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            DVec2::new(r * a.cos(), r * a.sin())
        });
        Self::new(points.collect(), Vec::new())
    }

    /// An I-section `height` × `width` with a `web` and two `flange`s, centred on the origin.
    pub fn i_section(height: f64, width: f64, web: f64, flange: f64) -> Result<Self, Refusal> {
        if ![height, width, web, flange].into_iter().all(positive) || web >= width || 2.0 * flange >= height {
            return Err(Refusal::NotPositive);
        }
        let (h, b, t, f) = (height / 2.0, width / 2.0, web / 2.0, flange);
        let p = DVec2::new;
        Self::new(
            vec![
                p(-b, -h),
                p(b, -h),
                p(b, -h + f),
                p(t, -h + f),
                p(t, h - f),
                p(b, h - f),
                p(b, h),
                p(-b, h),
                p(-b, h - f),
                p(-t, h - f),
                p(-t, -h + f),
                p(-b, -h + f),
            ],
            Vec::new(),
        )
    }

    /// The outline, counter-clockwise.
    pub fn outer(&self) -> &[DVec2] {
        &self.outer
    }

    /// The holes, each clockwise.
    pub fn holes(&self) -> &[Vec<DVec2>] {
        &self.holes
    }

    /// The outline, then the holes.
    pub fn rings(&self) -> impl Iterator<Item = &Vec<DVec2>> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }

    /// The area, holes taken off.
    pub fn area(&self) -> f64 {
        self.rings().map(|ring| signed_area(ring)).sum()
    }

    /// How far the profile reaches in direction `dir` (a unit vector) from the origin.
    pub fn support(&self, dir: DVec2) -> f64 {
        self.outer.iter().map(|p| p.dot(dir)).fold(f64::NEG_INFINITY, f64::max)
    }
}

/// The number of sides of a polygon within [`CHORD_TOLERANCE`] of a circle of radius `r`.
pub fn circle_segments(r: f64) -> usize {
    let half = (1.0 - CHORD_TOLERANCE / r).clamp(-1.0, 1.0).acos();
    ((std::f64::consts::PI / half.max(1e-9)).ceil() as usize).clamp(12, 256)
}

fn signed_area(ring: &[DVec2]) -> f64 {
    let n = ring.len();
    (0..n).map(|i| ring[i].perp_dot(ring[(i + 1) % n])).sum::<f64>() / 2.0
}

/// A ring without repeated points and without points on a straight line between their neighbours
/// (exact). A point where the ring turns back on itself is refused.
fn clean(ring: Vec<DVec2>) -> Result<Vec<DVec2>, Refusal> {
    if ring.iter().any(|p| !p.is_finite()) {
        return Err(Refusal::NotPositive);
    }
    let mut points: Vec<DVec2> = Vec::with_capacity(ring.len());
    for p in ring {
        if points.last() != Some(&p) {
            points.push(p);
        }
    }
    while points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    loop {
        let n = points.len();
        if n < 3 {
            return Err(if n == 0 { Refusal::TooFewPoints } else { Refusal::Degenerate });
        }
        let straight = (0..n).find(|&i| orient2d(points[(i + n - 1) % n], points[i], points[(i + 1) % n]) == Ordering::Equal);
        let Some(i) = straight else { return Ok(points) };
        let (before, at, after) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
        // on one line: a straight continuation is dropped; turning back is a fold
        if (at - before).dot(after - at) < 0.0 {
            return Err(if n == 3 { Refusal::Degenerate } else { Refusal::SelfIntersecting });
        }
        points.remove(i);
    }
}

/// A profile as its definition is kept: by kind and sizes (metres), as a polygon, or as a
/// catalogue profile (a steel profile of the library) by its designation and its shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileDef {
    Rectangle { width: f64, height: f64 },
    Circle { diameter: f64 },
    ISection { height: f64, width: f64, web: f64, flange: f64 },
    Polygon { outer: Vec<DVec2>, #[serde(default)] holes: Vec<Vec<DVec2>> },
    /// "HEA 200": the designation it is named by and its parametric shape, kept with it so the
    /// element is whole without the catalogue.
    Catalogue { designation: String, shape: bim_library::ProfileShape },
}

impl ProfileDef {
    /// The checked profile.
    pub fn profile(&self) -> Result<Profile, Refusal> {
        match self {
            ProfileDef::Rectangle { width, height } => Profile::rectangle(*width, *height),
            ProfileDef::Circle { diameter } => Profile::circle(*diameter),
            ProfileDef::ISection { height, width, web, flange } => Profile::i_section(*height, *width, *web, *flange),
            ProfileDef::Polygon { outer, holes } => Profile::new(outer.clone(), holes.clone()),
            ProfileDef::Catalogue { shape, .. } => {
                // the root fillets and round sections within the chord tolerance
                let (outer, holes) = shape.rings(CHORD_TOLERANCE).map_err(|_| Refusal::NotPositive)?;
                Profile::new(outer, holes)
            }
        }
    }

    /// The steel profile of the library's table with `designation` ("HEA 200").
    pub fn steel(designation: &str) -> Option<Self> {
        let p = bim_library::profile::steel(designation)?;
        Some(ProfileDef::Catalogue { designation: p.designation.clone(), shape: p.shape.clone() })
    }

    /// A short name, in millimetres: `200×300`, `Ø300`, `I300×150`, `polygon`.
    pub fn name(&self) -> String {
        let mm = |v: f64| format!("{}", (v * 1000.0).round());
        match self {
            ProfileDef::Rectangle { width, height } => format!("{}×{}", mm(*width), mm(*height)),
            ProfileDef::Circle { diameter } => format!("Ø{}", mm(*diameter)),
            ProfileDef::ISection { height, width, .. } => format!("I{}×{}", mm(*height), mm(*width)),
            ProfileDef::Polygon { .. } => "polygon".into(),
            ProfileDef::Catalogue { designation, .. } => designation.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn every_steel_profile_of_the_library_is_a_checked_profile() {
        for p in bim_library::profile::steel_profiles() {
            let def = ProfileDef::steel(&p.designation).unwrap();
            assert_eq!(def.name(), p.designation);
            let profile = def.profile().unwrap_or_else(|e| panic!("{}: {e}", p.designation));
            let exact = p.shape.props().unwrap().area;
            // round edges are chords within the chord tolerance, so a small tube has a little less
            assert!((profile.area() - exact).abs() / exact < 0.01, "{}: {} vs {exact}", p.designation, profile.area());
            assert_eq!(profile.holes().len(), usize::from(matches!(p.series, bim_library::Series::Shs | bim_library::Series::Rhs | bim_library::Series::Chs)));
        }
        assert!(ProfileDef::steel("HEA 210").is_none());
    }

    #[test]
    fn a_catalogue_profile_round_trips_with_its_designation() {
        let hea = ProfileDef::steel("HEA 200").unwrap();
        let json = serde_json::to_string(&hea).unwrap();
        assert!(json.starts_with("{\"kind\":\"catalogue\",\"designation\":\"HEA 200\""), "{json}");
        assert_eq!(serde_json::from_str::<ProfileDef>(&json).unwrap(), hea);
    }

    #[test]
    fn outlines_run_counter_clockwise_and_holes_clockwise() {
        let p = Profile::new(
            vec![v(0.0, 0.0), v(0.0, 4.0), v(4.0, 4.0), v(4.0, 0.0)],
            vec![vec![v(1.0, 1.0), v(2.0, 1.0), v(2.0, 2.0), v(1.0, 2.0)]],
        )
        .unwrap();
        assert_eq!(ring_orientation(p.outer()), Ordering::Greater);
        assert_eq!(ring_orientation(&p.holes()[0]), Ordering::Less);
        assert!((p.area() - 15.0).abs() < 1e-12);
    }

    #[test]
    fn repeated_and_straight_points_are_dropped() {
        let p = Profile::new(vec![v(0.0, 0.0), v(1.0, 0.0), v(2.0, 0.0), v(2.0, 2.0), v(2.0, 2.0), v(0.0, 2.0), v(0.0, 0.0)], Vec::new()).unwrap();
        assert_eq!(p.outer().len(), 4);
    }

    #[test]
    fn bad_outlines_are_refused() {
        assert_eq!(Profile::new(vec![v(0.0, 0.0), v(1.0, 0.0)], Vec::new()), Err(Refusal::Degenerate));
        assert_eq!(Profile::new(vec![v(0.0, 0.0), v(1.0, 1.0), v(2.0, 2.0)], Vec::new()), Err(Refusal::Degenerate));
        // a bow tie
        assert_eq!(Profile::new(vec![v(0.0, 0.0), v(2.0, 2.0), v(2.0, 0.0), v(0.0, 2.0)], Vec::new()), Err(Refusal::SelfIntersecting));
        // a spike that turns back
        assert_eq!(Profile::new(vec![v(0.0, 0.0), v(2.0, 0.0), v(3.0, 0.0), v(2.5, 0.0), v(2.0, 2.0)], Vec::new()), Err(Refusal::SelfIntersecting));
        let square = vec![v(0.0, 0.0), v(4.0, 0.0), v(4.0, 4.0), v(0.0, 4.0)];
        let outside = vec![v(5.0, 1.0), v(6.0, 1.0), v(6.0, 2.0)];
        assert_eq!(Profile::new(square.clone(), vec![outside]), Err(Refusal::HoleOutside));
        let touching = vec![v(0.0, 1.0), v(1.0, 1.0), v(1.0, 2.0)];
        assert_eq!(Profile::new(square, vec![touching]), Err(Refusal::SelfIntersecting));
        assert_eq!(Profile::rectangle(0.0, 1.0), Err(Refusal::NotPositive));
        assert_eq!(Profile::rectangle(f64::NAN, 1.0), Err(Refusal::NotPositive));
        assert_eq!(Profile::i_section(0.3, 0.15, 0.2, 0.01), Err(Refusal::NotPositive));
    }

    #[test]
    fn a_circle_stays_within_the_chord_tolerance() {
        let d = 0.3;
        let p = Profile::circle(d).unwrap();
        let n = p.outer().len();
        let sagitta = d / 2.0 * (1.0 - (std::f64::consts::PI / n as f64).cos());
        assert!(sagitta <= CHORD_TOLERANCE, "{n} sides, {sagitta}");
        assert!((p.area() - std::f64::consts::PI * 0.15 * 0.15).abs() < 0.15 * CHORD_TOLERANCE * 7.0);
    }

    #[test]
    fn an_i_section_has_its_area_and_reach() {
        let p = Profile::i_section(0.3, 0.15, 0.01, 0.02).unwrap();
        let area = 0.15 * 0.02 * 2.0 + 0.01 * (0.3 - 0.04);
        assert!((p.area() - area).abs() < 1e-12);
        assert!((p.support(DVec2::Y) - 0.15).abs() < 1e-12);
        assert!((p.support(DVec2::X) - 0.075).abs() < 1e-12);
    }

    #[test]
    fn definitions_round_trip_through_json() {
        for def in [
            ProfileDef::Rectangle { width: 0.2, height: 0.3 },
            ProfileDef::Circle { diameter: 0.1 },
            ProfileDef::ISection { height: 0.3, width: 0.15, web: 0.0071, flange: 0.0107 },
            ProfileDef::Polygon { outer: vec![v(0.0, 0.0), v(1.0, 0.0), v(0.0, 1.0)], holes: Vec::new() },
        ] {
            let json = serde_json::to_string(&def).unwrap();
            assert_eq!(serde_json::from_str::<ProfileDef>(&json).unwrap(), def, "{json}");
            def.profile().unwrap();
        }
        assert_eq!(ProfileDef::Rectangle { width: 0.2, height: 0.3 }.name(), "200×300");
    }
}
