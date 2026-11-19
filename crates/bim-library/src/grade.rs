//! Strength classes: what a material's grade says of its strength and stiffness.
//!
//! The timber classes are Project Ocondat's (`data/ocondat-timber-grades.json`, see the file's
//! `source` and `licence`; extracted by `tools/ocondat-grades/extract.py`), compiled in:
//! softwood C14 to C35 and hardwood D30 to D70 of EN 338, and homogeneous glued laminated
//! timber GL24h to GL36h of EN 14080. The values are as Ocondat has them, but for the density of
//! C18 (320 kg/m³ as EN 338 has it, where Ocondat has 300); where an edition of
//! a standard differs (the mean modulus of D30, of GL24h), the standard a project follows
//! decides.
//!
//! A material names its grade ([`crate::Material::grade`]): "C24" on a material is this class,
//! and gives a structural model its modulus and its density.

use serde::Deserialize;
use std::sync::OnceLock;

/// The classes as extracted from Ocondat.
const TIMBER: &str = include_str!("../data/ocondat-timber-grades.json");

/// What a timber class is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimberKind {
    Softwood,
    Hardwood,
    Glulam,
}

/// A strength class of timber: characteristic strengths and stiffness. Strengths and moduli in
/// N/mm², the characteristic density in kN/m³.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TimberGrade {
    /// "C24", "D40", "GL28h".
    pub grade: String,
    pub kind: TimberKind,
    /// Bending.
    pub fm_k: f64,
    /// Tension along and across the grain.
    pub ft_0_k: f64,
    pub ft_90_k: f64,
    /// Compression along and across the grain.
    pub fc_0_k: f64,
    pub fc_90_k: f64,
    /// Shear.
    pub fv_k: f64,
    /// The modulus of elasticity along the grain: its fifth percentile and its mean.
    pub e_0_u_k: f64,
    pub e_0_mean: f64,
    /// The mean modulus of elasticity across the grain.
    pub e_90_mean: f64,
    /// The mean shear modulus.
    pub g_mean: f64,
    /// The characteristic density, as a weight: kN/m³.
    pub rho_k: f64,
}

impl TimberGrade {
    /// The mean modulus of elasticity along the grain, in N/m².
    pub fn e(&self) -> f64 {
        self.e_0_mean * 1e6
    }

    /// The mean shear modulus, in N/m².
    pub fn g(&self) -> f64 {
        self.g_mean * 1e6
    }

    /// The characteristic density in kg/m³: Ocondat has it as a weight at 10 m/s² (C24: 3.5
    /// kN/m³ for the 350 kg/m³ of EN 338).
    pub fn density(&self) -> f64 {
        self.rho_k * 100.0
    }
}

#[derive(Deserialize)]
struct File {
    grades: Vec<TimberGrade>,
}

/// Every timber class: softwood, hardwood, then glued laminated timber, each rising.
pub fn timber_grades() -> &'static [TimberGrade] {
    static GRADES: OnceLock<Vec<TimberGrade>> = OnceLock::new();
    GRADES.get_or_init(|| serde_json::from_str::<File>(TIMBER).expect("the timber grades are compiled in").grades)
}

/// The timber class called `grade`, whatever its case ("c24", "gl28H").
pub fn timber_grade(grade: &str) -> Option<&'static TimberGrade> {
    let grade = grade.trim();
    timber_grades().iter().find(|g| g.grade.eq_ignore_ascii_case(grade))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timber_classes_are_those_of_the_standards() {
        let grades = timber_grades();
        let of = |kind| grades.iter().filter(|g| g.kind == kind).map(|g| g.grade.as_str()).collect::<Vec<_>>();
        assert_eq!(of(TimberKind::Softwood), ["C14", "C16", "C18", "C20", "C22", "C24", "C27", "C30", "C35"]);
        assert_eq!(of(TimberKind::Glulam), ["GL24h", "GL28h", "GL32h", "GL36h"]);
        assert!(of(TimberKind::Hardwood).starts_with(&["D30"]) && of(TimberKind::Hardwood).ends_with(&["D70"]));
        for g in grades {
            // a class is named after its bending strength, and what it bears rises with it
            let named: f64 = g.grade.trim_start_matches(|c: char| c.is_ascii_alphabetic()).trim_end_matches('h').parse().unwrap();
            assert_eq!(g.fm_k, named, "{}", g.grade);
            assert!(g.ft_90_k < g.ft_0_k && g.fc_90_k < g.fc_0_k && g.e_0_u_k < g.e_0_mean && g.e_90_mean < g.e_0_mean && g.g_mean < g.e_0_mean, "{}", g.grade);
            assert!(g.rho_k > 2.0 && g.rho_k < 12.0, "{}", g.grade);
        }
        // within softwood, hardwood and glulam a stronger class is no lighter
        for pair in grades.windows(2).filter(|pair| pair[0].kind == pair[1].kind) {
            assert!(pair[1].rho_k >= pair[0].rho_k && pair[1].e_0_mean >= pair[0].e_0_mean, "{} to {}", pair[0].grade, pair[1].grade);
        }
    }

    #[test]
    fn c24_is_the_c24_of_en_338() {
        let c24 = timber_grade("c24").expect("C24");
        assert_eq!((c24.fm_k, c24.ft_0_k, c24.fc_0_k, c24.fv_k, c24.e_0_mean, c24.g_mean), (24.0, 14.0, 21.0, 4.0, 11000.0, 690.0));
        assert_eq!((c24.e(), c24.g(), c24.density()), (11.0e9, 0.69e9, 350.0));
        assert_eq!(timber_grade(" GL28H ").map(|g| g.e_0_mean), Some(12600.0));
        assert!(timber_grade("S235").is_none() && timber_grade("").is_none());
    }
}
