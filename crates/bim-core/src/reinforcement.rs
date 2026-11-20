//! The reinforcement of a concrete member with a rectangular section: bars along its top and its
//! bottom and closed stirrups round them, given by their numbers, diameters and cover. From these
//! and the member's sizes come the bars to cut, with their lengths and weights: the bar schedule.
//!
//! A first measure of it, for quantities and drawings: the bars are straight and as long as the
//! member less its cover at both ends, without laps, hooks or bends; a stirrup is the section's
//! outline inside the cover, with two hooks. Editable detailing defaults, not a checked design.

use crate::profile::Profile;
use crate::Refusal;
use serde::{Deserialize, Serialize};

/// The density of reinforcing steel (kg/m³).
pub const STEEL_DENSITY: f64 = 7850.0;
/// The length of each of a stirrup's two hooks, in bar diameters.
pub const HOOK_DIAMETERS: f64 = 10.0;
/// The least clear distance between two bars beside each other (metres), where their own
/// diameter is not more.
pub const CLEAR: f64 = 0.02;

/// A row of equal bars: how many, and their diameter (metres).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bars {
    pub count: u32,
    pub diameter: f64,
}

/// A member's reinforcement (metres).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Reinforcement {
    /// The concrete over the stirrups, on every side and at both ends.
    pub cover: f64,
    /// The bars along the top and along the bottom of the section.
    pub top: Bars,
    pub bottom: Bars,
    /// The stirrups' diameter, and how far apart they stand.
    pub stirrup: f64,
    pub spacing: f64,
}

impl Default for Reinforcement {
    /// 30 mm cover, 2 Ø12 on top, 3 Ø16 below, stirrups Ø8 every 150 mm.
    fn default() -> Self {
        Self { cover: 0.03, top: Bars { count: 2, diameter: 0.012 }, bottom: Bars { count: 3, diameter: 0.016 }, stirrup: 0.008, spacing: 0.15 }
    }
}

/// What a bar is in its member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKind {
    Top,
    Bottom,
    Stirrup,
}

/// Equal bars of a member, as its bar schedule lists them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarRow {
    pub kind: BarKind,
    pub diameter: f64,
    pub count: u32,
    /// The length one bar is cut to (metres).
    pub length: f64,
}

impl BarRow {
    /// The weight of all bars of the row (kg).
    pub fn weight(&self) -> f64 {
        std::f64::consts::PI * self.diameter * self.diameter / 4.0 * self.length * f64::from(self.count) * STEEL_DENSITY
    }
}

impl Reinforcement {
    /// The bars of a member `length` long with section `profile`: its top bars, its bottom bars
    /// and its stirrups (a row with no bars is left out). Refused when the section is no
    /// rectangle, a size is not positive, or the bars do not fit beside each other in it.
    pub fn bars(&self, profile: &Profile, length: f64) -> Result<Vec<BarRow>, Refusal> {
        let positive = |v: f64| v.is_finite() && v > crate::LINEAR_TOLERANCE;
        if ![self.cover, self.stirrup, self.spacing, length].into_iter().all(positive) || [self.top, self.bottom].iter().any(|b| b.count > 0 && !positive(b.diameter)) {
            return Err(Refusal::NotPositive);
        }
        let outer = profile.outer();
        let lo = outer.iter().fold(glam::DVec2::splat(f64::INFINITY), |a, p| a.min(*p));
        let hi = outer.iter().fold(glam::DVec2::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
        let (width, height) = (hi.x - lo.x, hi.y - lo.y);
        if outer.len() != 4 || !profile.holes().is_empty() || (profile.area() - width * height).abs() > 1e-9 {
            return Err(Refusal::NotRectangular);
        }
        // inside the stirrups
        let (inner, tall) = (width - 2.0 * (self.cover + self.stirrup), height - 2.0 * (self.cover + self.stirrup));
        let straight = length - 2.0 * self.cover;
        if !positive(straight) || !positive(inner) || !positive(tall) {
            return Err(Refusal::BarsDoNotFit);
        }
        let mut rows = Vec::new();
        for (kind, bars) in [(BarKind::Top, self.top), (BarKind::Bottom, self.bottom)] {
            if bars.count == 0 {
                continue;
            }
            let needed = f64::from(bars.count) * bars.diameter + f64::from(bars.count - 1) * bars.diameter.max(CLEAR);
            if needed > inner + 1e-9 || bars.diameter > tall {
                return Err(Refusal::BarsDoNotFit);
            }
            rows.push(BarRow { kind, diameter: bars.diameter, count: bars.count, length: straight });
        }
        // a stirrup: round the section on its centre line, and its two hooks
        let (w, h) = (width - 2.0 * self.cover - self.stirrup, height - 2.0 * self.cover - self.stirrup);
        let count = (straight / self.spacing + 1e-9).floor() as u32 + 1;
        rows.push(BarRow { kind: BarKind::Stirrup, diameter: self.stirrup, count, length: 2.0 * (w + h) + 2.0 * HOOK_DIAMETERS * self.stirrup });
        Ok(rows)
    }

    /// Where the bars along the member lie in its section (the profile's own axes): the centre
    /// and the diameter of each, the top row first, each row spread evenly between the
    /// stirrup's legs (one bar alone in the middle). Refused as [`Reinforcement::bars`].
    pub fn centres(&self, profile: &Profile) -> Result<Vec<(glam::DVec2, f64)>, Refusal> {
        self.bars(profile, 1.0 + 2.0 * self.cover)?;
        let outer = profile.outer();
        let lo = outer.iter().fold(glam::DVec2::splat(f64::INFINITY), |a, p| a.min(*p));
        let hi = outer.iter().fold(glam::DVec2::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
        let inset = self.cover + self.stirrup;
        let mut centres = Vec::new();
        for (bars, top) in [(self.top, true), (self.bottom, false)] {
            let half = bars.diameter / 2.0;
            let y = if top { hi.y - inset - half } else { lo.y + inset + half };
            let (first, last) = (lo.x + inset + half, hi.x - inset - half);
            for i in 0..bars.count {
                let x = if bars.count == 1 { (first + last) / 2.0 } else { first + (last - first) * f64::from(i) / f64::from(bars.count - 1) };
                centres.push((glam::DVec2::new(x, y), bars.diameter));
            }
        }
        Ok(centres)
    }

    /// Where the stirrups stand along a member `length` long, from its start: as many as
    /// [`Reinforcement::bars`] counts, their spacing apart and in the middle of the length
    /// inside the cover. Empty where that length is not positive.
    pub fn stirrups(&self, length: f64) -> Vec<f64> {
        let straight = length - 2.0 * self.cover;
        if !(straight > 0.0 && self.spacing > crate::LINEAR_TOLERANCE) {
            return Vec::new();
        }
        let count = (straight / self.spacing + 1e-9).floor() as u32 + 1;
        let first = self.cover + (straight - f64::from(count - 1) * self.spacing) / 2.0;
        (0..count).map(|i| first + f64::from(i) * self.spacing).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_beam_lists_its_bars_with_lengths_and_weights() {
        // 300 × 500, 6 m long
        let section = Profile::rectangle(0.3, 0.5).unwrap();
        let rows = Reinforcement::default().bars(&section, 6.0).unwrap();
        assert_eq!(rows.iter().map(|r| (r.kind, r.count)).collect::<Vec<_>>(), [(BarKind::Top, 2), (BarKind::Bottom, 3), (BarKind::Stirrup, 40)]);
        // straight bars: the length less the cover at both ends
        assert!((rows[0].length - 5.94).abs() < 1e-12 && (rows[1].length - 5.94).abs() < 1e-12);
        // a stirrup: 2 × (232 + 432) on its centre line and two hooks of 80
        assert!((rows[2].length - (2.0 * (0.232 + 0.432) + 0.16)).abs() < 1e-12, "{}", rows[2].length);
        // Ø16 weighs 1.578 kg/m, Ø12 0.888 and Ø8 0.395
        assert!((rows[1].weight() - 3.0 * 5.94 * 1.578).abs() < 0.02, "{}", rows[1].weight());
        assert!((rows[0].weight() - 2.0 * 5.94 * 0.888).abs() < 0.02, "{}", rows[0].weight());
        assert!((rows[2].weight() - 40.0 * 1.488 * 0.395).abs() < 0.05, "{}", rows[2].weight());
    }

    #[test]
    fn the_bars_lie_inside_the_stirrups_and_the_stirrups_in_the_middle_of_the_length() {
        let section = Profile::rectangle(0.3, 0.5).unwrap();
        let r = Reinforcement::default();
        let centres = r.centres(&section).unwrap();
        let lo = section.outer().iter().fold(glam::DVec2::splat(f64::INFINITY), |a, p| a.min(*p));
        // two on top, 30 + 8 + 6 from the top and the sides; three below with one in the middle
        let at: Vec<(i64, i64, i64)> = centres.iter().map(|(c, d)| (((c.x - lo.x) * 1000.0).round() as i64, ((c.y - lo.y) * 1000.0).round() as i64, (d * 1000.0).round() as i64)).collect();
        assert_eq!(at, [(44, 456, 12), (256, 456, 12), (46, 46, 16), (150, 46, 16), (254, 46, 16)]);
        // 40 stirrups at 150 take 5.85 of the 5.94 inside the cover: 45 left at each end
        let stirrups = r.stirrups(6.0);
        assert_eq!(stirrups.len(), 40);
        assert!((stirrups[0] - 0.075).abs() < 1e-9 && (stirrups[39] - 5.925).abs() < 1e-9 && (stirrups[1] - stirrups[0] - 0.15).abs() < 1e-9, "{stirrups:?}");
        assert!(r.stirrups(0.05).is_empty());
        assert_eq!(r.centres(&Profile::circle(0.4).unwrap()), Err(Refusal::NotRectangular));
    }

    #[test]
    fn bars_that_do_not_fit_and_other_sections_are_refused() {
        let section = Profile::rectangle(0.2, 0.4).unwrap();
        let fits = Reinforcement { bottom: Bars { count: 3, diameter: 0.02 }, ..Default::default() };
        assert!(fits.bars(&section, 4.0).is_ok(), "3 Ø20 need 100 of the 124 inside the stirrups");
        let crowded = Reinforcement { bottom: Bars { count: 4, diameter: 0.02 }, ..Default::default() };
        assert_eq!(crowded.bars(&section, 4.0), Err(Refusal::BarsDoNotFit));
        assert_eq!(Reinforcement { cover: 0.2, ..Default::default() }.bars(&section, 4.0), Err(Refusal::BarsDoNotFit));
        assert_eq!(Reinforcement { spacing: 0.0, ..Default::default() }.bars(&section, 4.0), Err(Refusal::NotPositive));
        assert_eq!(Reinforcement::default().bars(&Profile::circle(0.4).unwrap(), 4.0), Err(Refusal::NotRectangular));
        // no top bars: only the bottom ones and the stirrups
        let rows = Reinforcement { top: Bars { count: 0, diameter: 0.0 }, ..Default::default() }.bars(&section, 4.0).unwrap();
        assert_eq!(rows.iter().map(|r| r.kind).collect::<Vec<_>>(), [BarKind::Bottom, BarKind::Stirrup]);
    }
}
