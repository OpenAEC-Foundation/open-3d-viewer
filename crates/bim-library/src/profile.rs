//! Profiles and linear types. A profile is one of IFC's parametric profiles, in metres, with the
//! middle of its bounding box on the element's axis, its width along u and its depth along v. Its
//! outline (with root fillets as arcs) and its area and second moments come from
//! `bim_structural::section`, so there is one implementation of profile geometry.
//!
//! The European steel profiles are Open BIM Studio's table (EN 10365, EN 10056-1, EN 10055,
//! EN 10210): IPE, IPN, HEA, HEB, HEM, UPN, UPE, equal and unequal angles, T, SHS, RHS and CHS.
//! Taken from the standards Open BIM Studio cites, where it drew a simpler shape: IPN and UPN have
//! tapered flanges, angles a toe radius of half the root radius, and hot-finished hollow sections
//! an inner corner radius of the wall thickness besides the outer one of 1.5 times it.

use crate::material::Material;
use crate::{Lang, LibraryError};
use bim_structural::section;
use bim_structural::Shape;
use glam::DVec2;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// A parametric profile, in metres (IFC's parametric profile definitions).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProfileShape {
    /// `width` along u, `depth` along v.
    Rectangle { width: f64, depth: f64 },
    Circle { diameter: f64 },
    /// An I or H section: depth `h`, flange width `b`, web `tw`, flange `tf` (halfway along a
    /// tapered flange), root radius `r`, flange slope in radians.
    I { h: f64, b: f64, tw: f64, tf: f64, r: f64, slope: f64 },
    /// A channel, its web at −u.
    U { h: f64, b: f64, tw: f64, tf: f64, r: f64, slope: f64 },
    /// An angle: leg `h` along v, leg `b` along u, thickness `t`, root radius `r1`, toe radius `r2`.
    L { h: f64, b: f64, t: f64, r1: f64, r2: f64 },
    /// A T section, its flange at +v.
    T { h: f64, b: f64, tw: f64, tf: f64, r: f64 },
    /// A rectangular or square hollow section.
    Rhs { h: f64, b: f64, t: f64, r_out: f64, r_in: f64 },
    /// A circular hollow section.
    Chs { d: f64, t: f64 },
}

/// A profile's area and second moments, about its centroid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfileProps {
    /// m²
    pub area: f64,
    /// About u (bending in the depth), m⁴.
    pub iu: f64,
    /// About v, m⁴.
    pub iv: f64,
}

impl ProfileShape {
    /// The same shape in the structural model's terms.
    pub fn structural(&self) -> Shape {
        match *self {
            ProfileShape::Rectangle { width, depth } => Shape::Rect { b: width, h: depth },
            ProfileShape::Circle { diameter } => Shape::Circle { d: diameter },
            ProfileShape::I { h, b, tw, tf, r, slope } => Shape::I { h, b, tw, tf, r, slope },
            ProfileShape::U { h, b, tw, tf, r, slope } => Shape::U { h, b, tw, tf, r1: r, r2: 0.0, slope },
            ProfileShape::L { h, b, t, r1, r2 } => Shape::L { h, b, t, r1, r2 },
            ProfileShape::T { h, b, tw, tf, r } => Shape::T { h, b, tw, tf, r },
            ProfileShape::Rhs { h, b, t, r_out, r_in } => Shape::Rhs { b, h, t, r_out, r_in },
            ProfileShape::Chs { d, t } => Shape::Chs { d, t },
        }
    }

    /// Checks its dimensions.
    pub fn check(&self) -> Result<(), LibraryError> {
        section::check(&self.structural()).map_err(|e| LibraryError::BadProfile(format!("{e:?}")))
    }

    /// Its width (along u) and depth (along v).
    pub fn size(&self) -> DVec2 {
        match *self {
            ProfileShape::Rectangle { width, depth } => DVec2::new(width, depth),
            ProfileShape::Circle { diameter: d } | ProfileShape::Chs { d, .. } => DVec2::splat(d),
            ProfileShape::I { h, b, .. } | ProfileShape::U { h, b, .. } | ProfileShape::L { h, b, .. } | ProfileShape::T { h, b, .. } | ProfileShape::Rhs { h, b, .. } => DVec2::new(b, h),
        }
    }

    /// Whether it is round (a circle or a circular hollow section).
    pub fn round(&self) -> bool {
        matches!(self, ProfileShape::Circle { .. } | ProfileShape::Chs { .. })
    }

    /// Its outline and holes, each a closed polygon (the closing point not repeated) with its arcs
    /// within `chord` metres; the outline counter-clockwise, holes clockwise.
    pub fn rings(&self, chord: f64) -> Result<(Vec<DVec2>, Vec<Vec<DVec2>>), LibraryError> {
        let outline = section::outline(&self.structural(), chord, 16).map_err(|e| LibraryError::BadProfile(format!("{e:?}")))?;
        let mut outer = Vec::new();
        let mut holes = Vec::new();
        for ring in outline.loops() {
            if ring.hole {
                holes.push(ring.points);
            } else {
                outer = ring.points;
            }
        }
        Ok((outer, holes))
    }

    /// Its area and second moments.
    pub fn props(&self) -> Result<ProfileProps, LibraryError> {
        let outline = section::outline(&self.structural(), section::CHORD_TOL, section::MAX_PER_QUARTER).map_err(|e| LibraryError::BadProfile(format!("{e:?}")))?;
        let p = section::props(&outline);
        Ok(ProfileProps { area: p.area, iu: p.iy, iv: p.iz })
    }

    /// Whether it is `other` within 0.01 mm in every dimension.
    pub fn same(&self, other: &ProfileShape) -> bool {
        let near = |a: f64, b: f64| (a - b).abs() <= 1e-5;
        let all = |a: &[f64], b: &[f64]| a.iter().zip(b).all(|(x, y)| near(*x, *y));
        use ProfileShape::*;
        match (self, other) {
            (Rectangle { width: a, depth: b }, Rectangle { width: c, depth: d }) => all(&[*a, *b], &[*c, *d]),
            (Circle { diameter: a }, Circle { diameter: b }) => near(*a, *b),
            (I { h, b, tw, tf, r, slope }, I { h: h2, b: b2, tw: tw2, tf: tf2, r: r2, slope: s2 }) | (U { h, b, tw, tf, r, slope }, U { h: h2, b: b2, tw: tw2, tf: tf2, r: r2, slope: s2 }) => {
                all(&[*h, *b, *tw, *tf, *r, *slope], &[*h2, *b2, *tw2, *tf2, *r2, *s2])
            }
            (L { h, b, t, r1, r2 }, L { h: h2, b: b2, t: t2, r1: r12, r2: r22 }) => all(&[*h, *b, *t, *r1, *r2], &[*h2, *b2, *t2, *r12, *r22]),
            (T { h, b, tw, tf, r }, T { h: h2, b: b2, tw: tw2, tf: tf2, r: r2 }) => all(&[*h, *b, *tw, *tf, *r], &[*h2, *b2, *tw2, *tf2, *r2]),
            (Rhs { h, b, t, r_out, r_in }, Rhs { h: h2, b: b2, t: t2, r_out: o2, r_in: i2 }) => all(&[*h, *b, *t, *r_out, *r_in], &[*h2, *b2, *t2, *o2, *i2]),
            (Chs { d, t }, Chs { d: d2, t: t2 }) => all(&[*d, *t], &[*d2, *t2]),
            _ => false,
        }
    }
}

/// The European steel series of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Series {
    Ipe,
    Ipn,
    Hea,
    Heb,
    Hem,
    Upn,
    Upe,
    LEqual,
    LUnequal,
    T,
    Shs,
    Rhs,
    Chs,
}

impl Series {
    pub const ALL: [Series; 13] = [
        Series::Ipe,
        Series::Ipn,
        Series::Hea,
        Series::Heb,
        Series::Hem,
        Series::Upn,
        Series::Upe,
        Series::LEqual,
        Series::LUnequal,
        Series::T,
        Series::Shs,
        Series::Rhs,
        Series::Chs,
    ];

    /// Its name in a designation.
    pub fn name(self) -> &'static str {
        match self {
            Series::Ipe => "IPE",
            Series::Ipn => "IPN",
            Series::Hea => "HEA",
            Series::Heb => "HEB",
            Series::Hem => "HEM",
            Series::Upn => "UPN",
            Series::Upe => "UPE",
            Series::LEqual | Series::LUnequal => "L",
            Series::T => "T",
            Series::Shs => "SHS",
            Series::Rhs => "RHS",
            Series::Chs => "CHS",
        }
    }

    /// The standard its dimensions come from.
    pub fn standard(self) -> &'static str {
        match self {
            Series::Ipe | Series::Ipn | Series::Hea | Series::Heb | Series::Hem | Series::Upn | Series::Upe => "EN 10365",
            Series::LEqual | Series::LUnequal => "EN 10056-1",
            Series::T => "EN 10055",
            Series::Shs | Series::Rhs | Series::Chs => "EN 10210",
        }
    }

    /// Whether its profiles are columns too (beams take every profile): the H sections and the
    /// hollow sections.
    pub fn column(self) -> bool {
        matches!(self, Series::Hea | Series::Heb | Series::Hem | Series::Shs | Series::Rhs | Series::Chs)
    }
}

/// A profile of the steel table.
#[derive(Debug, Clone, PartialEq)]
pub struct SteelProfile {
    pub series: Series,
    /// "HEA 200", "L 80x80x8", "SHS 100x100x5", "CHS 114.3x5".
    pub designation: String,
    pub shape: ProfileShape,
}

/// Millimetres in a steel designation: whole, or to one decimal ("114.3").
fn size(metres: f64) -> String {
    let tenths = (metres * 10_000.0).round() as i64;
    if tenths % 10 == 0 {
        (tenths / 10).to_string()
    } else {
        format!("{}.{}", tenths / 10, tenths % 10)
    }
}

/// Every profile of the steel table, in table order, read once.
pub fn steel_profiles() -> &'static [SteelProfile] {
    static PROFILES: OnceLock<Vec<SteelProfile>> = OnceLock::new();
    PROFILES.get_or_init(|| {
        let mut profiles = Vec::new();
        for &(series, rows) in TABLE {
            for row in rows.split(';').map(str::trim).filter(|r| !r.is_empty()) {
                // nominal size, h, b, tw, tf, r (mm), column flag
                let v: Vec<f64> = row.split_whitespace().map(|f| f.parse().expect("a number in the steel table")).collect();
                let (nominal, h, b, tw, tf, r) = (v[0], v[1] / 1000.0, v[2] / 1000.0, v[3] / 1000.0, v[4] / 1000.0, v[5] / 1000.0);
                let shape = match series {
                    Series::Ipe | Series::Hea | Series::Heb | Series::Hem => ProfileShape::I { h, b, tw, tf, r, slope: 0.0 },
                    // DIN 1025-1: flanges taper at 14 %
                    Series::Ipn => ProfileShape::I { h, b, tw, tf, r, slope: 0.14f64.atan() },
                    // DIN 1026-1: 8 % up to 300 mm deep, 5 % above
                    Series::Upn => ProfileShape::U { h, b, tw, tf, r, slope: if h <= 0.3 + 1e-9 { 0.08f64.atan() } else { 0.05f64.atan() } },
                    Series::Upe => ProfileShape::U { h, b, tw, tf, r, slope: 0.0 },
                    Series::LEqual | Series::LUnequal => ProfileShape::L { h, b, t: tw, r1: r, r2: r / 2.0 },
                    Series::T => ProfileShape::T { h, b, tw, tf, r },
                    // the table holds the wall as tw and the outer radius (1.5 t) as tf
                    Series::Shs | Series::Rhs => ProfileShape::Rhs { h, b, t: tw, r_out: tf, r_in: tw },
                    Series::Chs => ProfileShape::Chs { d: b, t: tw },
                };
                let designation = match series {
                    Series::LEqual | Series::LUnequal | Series::Shs | Series::Rhs => format!("{} {}x{}x{}", series.name(), size(h), size(b), size(tw)),
                    Series::Chs => format!("CHS {}x{}", size(b), size(tw)),
                    _ => format!("{} {}", series.name(), nominal),
                };
                profiles.push(SteelProfile { series, designation, shape });
            }
        }
        profiles
    })
}

/// The profile of the steel table with `designation` ("HEA 200").
pub fn steel(designation: &str) -> Option<&'static SteelProfile> {
    steel_profiles().iter().find(|p| p.designation == designation)
}

/// What a linear element is, and so its IFC class: Open BIM Studio's linear roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinearRole {
    Column,
    Beam,
    Member,
    Mullion,
    Skirting,
    Pipe,
    Duct,
    CableTray,
    Pile,
}

impl LinearRole {
    pub const ALL: [LinearRole; 9] = [
        LinearRole::Column,
        LinearRole::Beam,
        LinearRole::Member,
        LinearRole::Mullion,
        LinearRole::Skirting,
        LinearRole::Pipe,
        LinearRole::Duct,
        LinearRole::CableTray,
        LinearRole::Pile,
    ];

    /// Its English name, as data.
    pub fn name(self) -> &'static str {
        match self {
            LinearRole::Column => "Column",
            LinearRole::Beam => "Beam",
            LinearRole::Member => "Member",
            LinearRole::Mullion => "Mullion",
            LinearRole::Skirting => "Skirting",
            LinearRole::Pipe => "Pipe",
            LinearRole::Duct => "Duct",
            LinearRole::CableTray => "Cable tray",
            LinearRole::Pile => "Pile",
        }
    }

    /// The IFC class (IFC4.3) of its elements, its type's class and the predefined type.
    pub fn ifc(self) -> (&'static str, &'static str, &'static str) {
        match self {
            LinearRole::Column => ("IfcColumn", "IfcColumnType", "COLUMN"),
            LinearRole::Beam => ("IfcBeam", "IfcBeamType", "BEAM"),
            LinearRole::Member => ("IfcMember", "IfcMemberType", "MEMBER"),
            LinearRole::Mullion => ("IfcMember", "IfcMemberType", "MULLION"),
            LinearRole::Skirting => ("IfcCovering", "IfcCoveringType", "SKIRTINGBOARD"),
            LinearRole::Pipe => ("IfcPipeSegment", "IfcPipeSegmentType", "RIGIDSEGMENT"),
            LinearRole::Duct => ("IfcDuctSegment", "IfcDuctSegmentType", "RIGIDSEGMENT"),
            LinearRole::CableTray => ("IfcCableCarrierSegment", "IfcCableCarrierSegmentType", "CABLETRAYSEGMENT"),
            LinearRole::Pile => ("IfcPile", "IfcPileType", "NOTDEFINED"),
        }
    }
}

/// A linear type: a role, a material and a profile; a steel designation when it is a profile of
/// the steel table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinearType {
    /// Stable and never shown, as a build-up's.
    pub id: String,
    pub role: LinearRole,
    pub material: String,
    pub profile: ProfileShape,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub designation: Option<String>,
}

impl LinearType {
    pub fn new(id: &str, role: LinearRole, material: &str, profile: ProfileShape) -> Self {
        Self { id: id.into(), role, material: material.into(), profile, designation: None }
    }

    /// A type of `role` of the steel profile `designation`, of steel.
    pub fn steel(id: &str, role: LinearRole, designation: &str) -> Option<Self> {
        let p = steel(designation)?;
        Some(Self { id: id.into(), role, material: "steel".into(), profile: p.shape.clone(), designation: Some(p.designation.clone()) })
    }

    /// Checks it against `materials`: an id, a known material, a profile that checks.
    pub fn check(&self, materials: &[Material]) -> Result<(), LibraryError> {
        if self.id.trim().is_empty() {
            return Err(LibraryError::NoKey);
        }
        if !materials.iter().any(|m| m.key == self.material) {
            return Err(LibraryError::UnknownMaterial(self.material.clone()));
        }
        self.profile.check()
    }

    /// Its name by Open BIM Studio's rule: its steel designation ("HEA 200"), else its material
    /// and size: "Beton 300x300", "Beton rond 300". The role is the group it is listed in, so it is
    /// not part of the name.
    pub fn rule_name(&self, materials: &[Material], lang: Lang) -> String {
        if let Some(designation) = &self.designation {
            return designation.clone();
        }
        let material = materials.iter().find(|m| m.key == self.material).map_or(self.material.as_str(), |m| m.name(lang));
        let mut chars = material.chars();
        let material: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
        let s = self.profile.size();
        let mm = |v: f64| crate::millimetres(v, lang);
        if self.profile.round() {
            let round = match lang {
                Lang::Nl => "rond",
                Lang::En => "round",
                Lang::De => "rund",
                Lang::Fr => "rond",
            };
            format!("{material} {round} {}", mm(s.x))
        } else {
            format!("{material} {}x{}", mm(s.x), mm(s.y))
        }
    }
}

/// The linear types a library starts with: Open BIM Studio's columns, beams, a mullion, a
/// skirting and a pipe.
pub fn starter() -> Vec<LinearType> {
    use LinearRole::*;
    let rect = |w: f64, d: f64| ProfileShape::Rectangle { width: w, depth: d };
    vec![
        LinearType::new("column.concrete-300x300", Column, "concrete", rect(0.3, 0.3)),
        LinearType::new("column.concrete-round-300", Column, "concrete", ProfileShape::Circle { diameter: 0.3 }),
        LinearType::new("column.steel-200x200", Column, "steel", rect(0.2, 0.2)),
        LinearType::new("column.timber-150x150", Column, "timber", rect(0.15, 0.15)),
        LinearType::new("beam.concrete-300x500", Beam, "concrete", rect(0.3, 0.5)),
        LinearType::new("beam.steel-200x400", Beam, "steel", rect(0.2, 0.4)),
        LinearType::new("beam.timber-75x225", Beam, "timber", rect(0.075, 0.225)),
        LinearType::new("mullion.aluminium-50x100", Mullion, "aluminium", rect(0.05, 0.1)),
        LinearType::new("skirting.timber-12x70", Skirting, "timber", rect(0.012, 0.07)),
        LinearType::new("pipe.generic-110", Pipe, "generic", ProfileShape::Circle { diameter: 0.11 }),
    ]
}

/// Open BIM Studio's steel table, per series: nominal size (0 when the designation is its
/// dimensions), h, b, tw, tf, r in millimetres, and a column flag. Angles have their thickness as
/// tw and tf; hollow sections their wall as tw and their outer corner radius (1.5 t) as tf.
const TABLE: &[(Series, &str)] = &[
    (
        Series::Ipe,
        "80 80 46 3.8 5.2 5 0; 100 100 55 4.1 5.7 7 0; 120 120 64 4.4 6.3 7 0; 140 140 73 4.7 6.9 7 0; 160 160 82 5 7.4 9 0; 180 180 91 5.3 8 9 0;\
         200 200 100 5.6 8.5 12 0; 220 220 110 5.9 9.2 12 0; 240 240 120 6.2 9.8 15 0; 270 270 135 6.6 10.2 15 0; 300 300 150 7.1 10.7 15 0; 330 330 160 7.5 11.5 18 0;\
         360 360 170 8 12.7 18 0; 400 400 180 8.6 13.5 21 0; 450 450 190 9.4 14.6 21 0; 500 500 200 10.2 16 21 0; 550 550 210 11.1 17.2 24 0; 600 600 220 12 19 24 0",
    ),
    (
        Series::Ipn,
        "80 80 42 3.9 5.9 3.9 0; 100 100 50 4.5 6.8 4.5 0; 120 120 58 5.1 7.7 5.1 0; 140 140 66 5.7 8.6 5.7 0; 160 160 74 6.3 9.5 6.3 0; 180 180 82 6.9 10.4 6.9 0;\
         200 200 90 7.5 11.3 7.5 0; 220 220 98 8.1 12.2 8.1 0; 240 240 106 8.7 13.1 8.7 0; 260 260 113 9.4 14.1 9.4 0; 280 280 119 10.1 15.2 10.1 0; 300 300 125 10.8 16.2 10.8 0;\
         320 320 131 11.5 17.3 11.5 0; 340 340 137 12.2 18.3 12.2 0; 360 360 143 13 19.5 13 0; 380 380 149 13.7 20.5 13.7 0; 400 400 155 14.4 21.6 14.4 0; 450 450 170 16.2 24.3 16.2 0;\
         500 500 185 18 27 18 0; 550 550 200 19 30 19 0; 600 600 215 21.6 32.4 21.6 0",
    ),
    (
        Series::Hea,
        "100 96 100 5 8 12 1; 120 114 120 5 8 12 1; 140 133 140 5.5 8.5 12 1; 160 152 160 6 9 15 1; 180 171 180 6 9.5 15 1; 200 190 200 6.5 10 18 1;\
         220 210 220 7 11 18 1; 240 230 240 7.5 12 21 1; 260 250 260 7.5 12.5 24 1; 280 270 280 8 13 24 1; 300 290 300 8.5 14 27 1; 320 310 300 9 15.5 27 1;\
         340 330 300 9.5 16.5 27 1; 360 350 300 10 17.5 27 1; 400 390 300 11 19 27 1; 450 440 300 11.5 21 27 1; 500 490 300 12 23 27 1; 550 540 300 12.5 24 27 1;\
         600 590 300 13 25 27 1; 650 640 300 13.5 26 27 1; 700 690 300 14.5 27 27 1; 800 790 300 15 28 30 1; 900 890 300 16 30 30 1; 1000 990 300 16.5 31 30 1",
    ),
    (
        Series::Heb,
        "100 100 100 6 10 12 1; 120 120 120 6.5 11 12 1; 140 140 140 7 12 12 1; 160 160 160 8 13 15 1; 180 180 180 8.5 14 15 1; 200 200 200 9 15 18 1;\
         220 220 220 9.5 16 18 1; 240 240 240 10 17 21 1; 260 260 260 10 17.5 24 1; 280 280 280 10.5 18 24 1; 300 300 300 11 19 27 1; 320 320 300 11.5 20.5 27 1;\
         340 340 300 12 21.5 27 1; 360 360 300 12.5 22.5 27 1; 400 400 300 13.5 24 27 1; 450 450 300 14 26 27 1; 500 500 300 14.5 28 27 1; 550 550 300 15 29 27 1;\
         600 600 300 15.5 30 27 1; 650 650 300 16 31 27 1; 700 700 300 17 32 27 1; 800 800 300 17.5 33 30 1; 900 900 300 18.5 35 30 1; 1000 1000 300 19 36 30 1",
    ),
    (
        Series::Hem,
        "100 120 106 12 20 12 1; 120 140 126 12.5 21 12 1; 140 160 146 13 22 12 1; 160 180 166 14 23 15 1; 180 200 186 14.5 24 15 1; 200 220 206 15 25 18 1;\
         220 240 226 15.5 26 18 1; 240 270 248 18 32 21 1; 260 290 268 18 32.5 24 1; 280 310 288 18.5 33 24 1; 300 340 310 21 39 27 1; 320 359 309 21 40 27 1;\
         340 377 309 21 40 27 1; 360 395 308 21 40 27 1; 400 432 307 21 40 27 1; 450 478 307 21 40 27 1; 500 524 306 21 40 27 1; 550 572 306 21 40 27 1;\
         600 620 305 21 40 27 1; 650 668 305 21 40 27 1; 700 716 304 21 40 27 1; 800 814 303 21 40 30 1; 900 910 302 21 40 30 1; 1000 1008 302 21 40 30 1",
    ),
    (
        Series::Upn,
        "50 50 38 5 7 7 0; 65 65 42 5.5 7.5 7.5 0; 80 80 45 6 8 8 0; 100 100 50 6 8.5 8.5 0; 120 120 55 7 9 9 0; 140 140 60 7 10 10 0;\
         160 160 65 7.5 10.5 10.5 0; 180 180 70 8 11 11 0; 200 200 75 8.5 11.5 11.5 0; 220 220 80 9 12.5 12.5 0; 240 240 85 9.5 13 13 0; 260 260 90 10 14 14 0;\
         280 280 95 10 15 15 0; 300 300 100 10 16 16 0; 320 320 100 14 17.5 17.5 0; 350 350 100 14 16 16 0; 380 380 102 13.5 16 16 0; 400 400 110 14 18 18 0",
    ),
    (
        Series::Upe,
        "80 80 50 4 7 10 0; 100 100 55 4.5 7.5 10 0; 120 120 60 5 8 12 0; 140 140 65 5 9 12 0; 160 160 70 5.5 9.5 12 0; 180 180 75 5.5 10.5 12 0;\
         200 200 80 6 11 13 0; 220 220 85 6.5 12 13 0; 240 240 90 7 12.5 15 0; 270 270 95 7.5 13.5 15 0; 300 300 100 9.5 15 15 0; 330 330 105 11 16 18 0;\
         360 360 110 12 17 18 0; 400 400 115 13.5 18 18 0",
    ),
    (
        Series::LEqual,
        "0 20 20 3 3 3.5 0; 0 25 25 3 3 3.5 0; 0 25 25 4 4 3.5 0; 0 30 30 3 3 5 0; 0 30 30 4 4 5 0; 0 35 35 4 4 5 0;\
         0 40 40 4 4 6 0; 0 40 40 5 5 6 0; 0 45 45 4.5 4.5 7 0; 0 50 50 5 5 7 0; 0 50 50 6 6 7 0; 0 60 60 6 6 8 0;\
         0 60 60 8 8 8 0; 0 65 65 7 7 9 0; 0 70 70 7 7 9 0; 0 75 75 8 8 9 0; 0 80 80 8 8 10 0; 0 80 80 10 10 10 0;\
         0 90 90 9 9 11 0; 0 100 100 8 8 12 0; 0 100 100 10 10 12 0; 0 100 100 12 12 12 0; 0 120 120 10 10 13 0; 0 120 120 12 12 13 0;\
         0 150 150 12 12 16 0; 0 150 150 15 15 16 0; 0 160 160 15 15 17 0; 0 180 180 18 18 18 0; 0 200 200 16 16 18 0; 0 200 200 20 20 18 0;\
         0 250 250 25 25 18 0; 0 250 250 35 35 18 0",
    ),
    (
        Series::LUnequal,
        "0 30 20 3 3 4 0; 0 40 20 4 4 4 0; 0 40 25 4 4 4 0; 0 45 30 4 4 4.5 0; 0 50 30 5 5 5 0; 0 60 30 5 5 6 0;\
         0 60 40 5 5 6 0; 0 60 40 6 6 6 0; 0 65 50 5 5 6 0; 0 70 50 6 6 7 0; 0 75 50 6 6 7 0; 0 75 50 8 8 7 0;\
         0 80 40 6 6 7 0; 0 80 60 7 7 8 0; 0 100 50 6 6 8 0; 0 100 50 8 8 8 0; 0 100 65 7 7 10 0; 0 100 75 8 8 10 0;\
         0 120 80 8 8 11 0; 0 120 80 10 10 11 0; 0 125 75 8 8 11 0; 0 150 75 9 9 12 0; 0 150 90 10 10 12 0; 0 150 100 10 10 12 0;\
         0 200 100 10 10 15 0; 0 200 100 12 12 15 0; 0 200 150 12 12 15 0; 0 200 150 15 15 15 0",
    ),
    (
        Series::T,
        "30 30 30 4 4 0 0; 35 35 35 4.5 4.5 0 0; 40 40 40 5 5 0 0; 50 50 50 6 6 0 0; 60 60 60 7 7 0 0; 70 70 70 8 8 0 0;\
         80 80 80 9 9 0 0; 100 100 100 11 11 0 0; 120 120 120 13 13 0 0; 140 140 140 15 15 0 0",
    ),
    (
        Series::Shs,
        "0 40 40 3 4.5 0 1; 0 40 40 4 6 0 1; 0 50 50 3 4.5 0 1; 0 50 50 4 6 0 1; 0 50 50 5 7.5 0 1; 0 60 60 3 4.5 0 1;\
         0 60 60 4 6 0 1; 0 60 60 5 7.5 0 1; 0 70 70 4 6 0 1; 0 70 70 5 7.5 0 1; 0 80 80 4 6 0 1; 0 80 80 5 7.5 0 1;\
         0 80 80 6 9 0 1; 0 90 90 5 7.5 0 1; 0 100 100 4 6 0 1; 0 100 100 5 7.5 0 1; 0 100 100 6 9 0 1; 0 100 100 8 12 0 1;\
         0 100 100 10 15 0 1; 0 120 120 5 7.5 0 1; 0 120 120 6 9 0 1; 0 120 120 8 12 0 1; 0 120 120 10 15 0 1; 0 140 140 6 9 0 1;\
         0 140 140 8 12 0 1; 0 140 140 10 15 0 1; 0 150 150 6 9 0 1; 0 150 150 8 12 0 1; 0 150 150 10 15 0 1; 0 160 160 6 9 0 1;\
         0 160 160 8 12 0 1; 0 160 160 10 15 0 1; 0 180 180 8 12 0 1; 0 180 180 10 15 0 1; 0 200 200 6 9 0 1; 0 200 200 8 12 0 1;\
         0 200 200 10 15 0 1; 0 200 200 12.5 18.7 0 1; 0 250 250 8 12 0 1; 0 250 250 10 15 0 1; 0 250 250 12.5 18.7 0 1; 0 300 300 10 15 0 1;\
         0 300 300 12.5 18.7 0 1",
    ),
    (
        Series::Rhs,
        "0 50 30 3 4.5 0 1; 0 60 40 3 4.5 0 1; 0 60 40 4 6 0 1; 0 80 40 4 6 0 1; 0 80 60 4 6 0 1; 0 100 50 4 6 0 1;\
         0 100 50 5 7.5 0 1; 0 100 60 4 6 0 1; 0 100 60 5 7.5 0 1; 0 120 60 5 7.5 0 1; 0 120 80 5 7.5 0 1; 0 120 80 6 9 0 1;\
         0 140 80 6 9 0 1; 0 150 100 6 9 0 1; 0 150 100 8 12 0 1; 0 160 80 6 9 0 1; 0 200 100 6 9 0 1; 0 200 100 8 12 0 1;\
         0 200 100 10 15 0 1; 0 250 150 8 12 0 1; 0 250 150 10 15 0 1; 0 300 200 10 15 0 1; 0 300 200 12.5 18.7 0 1",
    ),
    (
        Series::Chs,
        "0 33.7 33.7 3.2 0 0 1; 0 42.4 42.4 3.2 0 0 1; 0 48.3 48.3 3.2 0 0 1; 0 48.3 48.3 4 0 0 1; 0 60.3 60.3 3.2 0 0 1; 0 60.3 60.3 4 0 0 1;\
         0 76.1 76.1 3.2 0 0 1; 0 76.1 76.1 4 0 0 1; 0 76.1 76.1 5 0 0 1; 0 88.9 88.9 4 0 0 1; 0 88.9 88.9 5 0 0 1; 0 88.9 88.9 6.3 0 0 1;\
         0 114.3 114.3 4 0 0 1; 0 114.3 114.3 5 0 0 1; 0 114.3 114.3 6.3 0 0 1; 0 139.7 139.7 5 0 0 1; 0 139.7 139.7 6.3 0 0 1; 0 139.7 139.7 8 0 0 1;\
         0 168.3 168.3 5 0 0 1; 0 168.3 168.3 6.3 0 0 1; 0 168.3 168.3 8 0 0 1; 0 168.3 168.3 10 0 0 1; 0 193.7 193.7 6.3 0 0 1; 0 193.7 193.7 8 0 0 1;\
         0 193.7 193.7 10 0 0 1; 0 219.1 219.1 6.3 0 0 1; 0 219.1 219.1 8 0 0 1; 0 219.1 219.1 10 0 0 1; 0 219.1 219.1 12.5 0 0 1; 0 244.5 244.5 8 0 0 1;\
         0 244.5 244.5 10 0 0 1; 0 273 273 8 0 0 1; 0 273 273 10 0 0 1; 0 273 273 12.5 0 0 1; 0 323.9 323.9 8 0 0 1; 0 323.9 323.9 10 0 0 1;\
         0 323.9 323.9 12.5 0 0 1; 0 355.6 355.6 10 0 0 1; 0 355.6 355.6 12.5 0 0 1; 0 406.4 406.4 10 0 0 1; 0 406.4 406.4 12.5 0 0 1; 0 457 457 10 0 0 1;\
         0 457 457 12.5 0 0 1; 0 508 508 12.5 0 0 1",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material;
    use std::collections::HashSet;

    #[test]
    fn the_steel_table_has_every_series_and_every_row_checks() {
        let profiles = steel_profiles();
        assert_eq!(profiles.len(), 323);
        let designations: HashSet<&str> = profiles.iter().map(|p| p.designation.as_str()).collect();
        assert_eq!(designations.len(), profiles.len(), "designations are unique");
        for series in Series::ALL {
            assert!(profiles.iter().any(|p| p.series == series), "{series:?}");
        }
        for p in profiles {
            p.shape.check().unwrap_or_else(|e| panic!("{}: {e}", p.designation));
            assert!(p.shape.props().unwrap().area > 0.0, "{}", p.designation);
        }
    }

    #[test]
    fn the_column_flags_of_the_table_follow_the_series() {
        for &(series, rows) in TABLE {
            for row in rows.split(';').map(str::trim).filter(|r| !r.is_empty()) {
                let flag = row.split_whitespace().last().unwrap() == "1";
                assert_eq!(flag, series.column(), "{series:?} {row}");
            }
        }
    }

    #[test]
    fn designations_follow_open_bim_studio() {
        for name in ["IPE 200", "IPN 80", "HEA 1000", "HEM 100", "UPN 50", "UPE 400", "L 80x80x8", "L 100x50x8", "T 50", "SHS 100x100x5", "SHS 200x200x12.5", "RHS 200x100x10", "CHS 114.3x5", "CHS 508x12.5"] {
            assert!(steel(name).is_some(), "{name}");
        }
        assert!(steel("HEA 210").is_none());
        let hea = steel("HEA 200").unwrap();
        assert_eq!(hea.shape.size(), DVec2::new(0.2, 0.19), "HEA 200 is 190 deep");
        assert_eq!(hea.series.standard(), "EN 10365");
    }

    #[test]
    fn areas_match_the_published_values() {
        // cm² from the EN product tables; IPN and UPN have no toe radius here, so a little more
        let published = [
            ("IPE 200", 28.5, 0.005),
            ("HEA 200", 53.8, 0.005),
            ("HEB 300", 149.1, 0.005),
            ("IPN 200", 33.4, 0.015),
            ("UPN 200", 32.2, 0.015),
            ("L 80x80x8", 12.3, 0.01),
            // hot finished (EN 10210-2); 18.4 is the cold-formed one of EN 10219
            ("SHS 100x100x5", 18.7, 0.005),
            ("CHS 114.3x5", 17.2, 0.005),
        ];
        for (name, cm2, tolerance) in published {
            let area = steel(name).unwrap().shape.props().unwrap().area * 1e4;
            assert!((area - cm2).abs() / cm2 <= tolerance, "{name}: {area:.2} cm² vs {cm2}");
        }
        let ipe = steel("IPE 200").unwrap().shape.props().unwrap();
        // published second moments: 1943 cm⁴ about y (strong axis), 142 cm⁴ about z
        assert!((ipe.iu * 1e8 - 1943.0).abs() / 1943.0 < 0.01, "{}", ipe.iu * 1e8);
        assert!((ipe.iv * 1e8 - 142.0).abs() / 142.0 < 0.01, "{}", ipe.iv * 1e8);
    }

    #[test]
    fn ipn_and_upn_flanges_taper() {
        let ProfileShape::I { slope, .. } = steel("IPN 200").unwrap().shape else { panic!() };
        assert!((slope.tan() - 0.14).abs() < 1e-12);
        let ProfileShape::U { slope, .. } = steel("UPN 300").unwrap().shape else { panic!() };
        assert!((slope.tan() - 0.08).abs() < 1e-12);
        let ProfileShape::U { slope, .. } = steel("UPN 320").unwrap().shape else { panic!() };
        assert!((slope.tan() - 0.05).abs() < 1e-12);
        let ProfileShape::U { slope, .. } = steel("UPE 200").unwrap().shape else { panic!() };
        assert_eq!(slope, 0.0);
    }

    #[test]
    fn a_profile_has_rings_within_its_size() {
        let (outer, holes) = steel("SHS 100x100x5").unwrap().shape.rings(1e-4).unwrap();
        assert_eq!(holes.len(), 1);
        let area = |r: &[DVec2]| (0..r.len()).map(|i| r[i].perp_dot(r[(i + 1) % r.len()])).sum::<f64>() / 2.0;
        assert!(area(&outer) > 0.0 && area(&holes[0]) < 0.0, "outline counter-clockwise, hole clockwise");
        assert!(outer.iter().all(|p| p.x.abs() <= 0.05 + 1e-12 && p.y.abs() <= 0.05 + 1e-12));
        let (outer, holes) = steel("HEA 200").unwrap().shape.rings(1e-4).unwrap();
        assert!(holes.is_empty() && outer.len() > 12, "an H with its root fillets");
    }

    #[test]
    fn linear_types_are_named_by_rule() {
        let materials = material::starter();
        let types = starter();
        let name = |id: &str, lang| types.iter().find(|t| t.id == id).unwrap().rule_name(&materials, lang);
        assert_eq!(name("column.concrete-300x300", Lang::Nl), "Beton 300x300");
        assert_eq!(name("column.concrete-round-300", Lang::Nl), "Beton rond 300");
        assert_eq!(name("column.concrete-round-300", Lang::De), "Beton rund 300");
        assert_eq!(name("skirting.timber-12x70", Lang::En), "Timber 12x70");
        let hea = LinearType::steel("column.hea-200", LinearRole::Column, "HEA 200").unwrap();
        assert_eq!(hea.rule_name(&materials, Lang::Fr), "HEA 200");
        assert_eq!(hea.material, "steel");
        assert!(LinearType::steel("x", LinearRole::Beam, "HEA 210").is_none());
    }

    #[test]
    fn linear_types_are_checked() {
        let materials = material::starter();
        for t in starter() {
            assert_eq!(t.check(&materials), Ok(()), "{}", t.id);
        }
        let bad = LinearType::new("x", LinearRole::Beam, "concrete", ProfileShape::Rectangle { width: 0.0, depth: 0.3 });
        assert!(matches!(bad.check(&materials), Err(LibraryError::BadProfile(_))));
        let unknown = LinearType::new("x", LinearRole::Beam, "cheese", ProfileShape::Circle { diameter: 0.1 });
        assert_eq!(unknown.check(&materials), Err(LibraryError::UnknownMaterial("cheese".into())));
    }

    #[test]
    fn roles_have_their_ifc_classes() {
        assert_eq!(LinearRole::Column.ifc(), ("IfcColumn", "IfcColumnType", "COLUMN"));
        assert_eq!(LinearRole::Mullion.ifc().2, "MULLION");
        assert_eq!(LinearRole::Pipe.ifc().0, "IfcPipeSegment");
    }

    #[test]
    fn shapes_round_trip_and_compare_within_a_hundredth_of_a_millimetre() {
        let hea = steel("HEA 200").unwrap().shape.clone();
        let json = serde_json::to_string(&hea).unwrap();
        assert!(json.starts_with("{\"kind\":\"i\""), "{json}");
        assert_eq!(serde_json::from_str::<ProfileShape>(&json).unwrap(), hea);
        let ProfileShape::I { h, b, tw, tf, r, slope } = hea.clone() else { panic!() };
        assert!(hea.same(&ProfileShape::I { h: h + 5e-6, b, tw, tf, r, slope }));
        assert!(!hea.same(&ProfileShape::I { h: h + 2e-5, b, tw, tf, r, slope }));
        assert!(!hea.same(&ProfileShape::U { h, b, tw, tf, r, slope }));
    }
}
