//! Section geometry: outlines, properties and cardinal points.
//!
//! Outlines are in profile `(u, v)` with the origin at the bounding-box centre, as IFC
//! parametric profiles. They keep fillets as exact circular arcs, so [`props`] integrates the
//! exact shape (Green's theorem along lines and arcs); [`Outline2::loops`] tessellates for
//! display within the chord tolerance.
//!
//! Orientation of the parametric shapes (u to the right, v up, as in the IFC4 figures):
//!
//! - `I`, `AsymI`: web on v; `AsymI` has the `bt` flange at +v.
//! - `U`, `C`: web at −u, flanges towards +u. `C` lips turn towards the web's middle.
//! - `L`: heel at (−b/2, −h/2); leg of depth `h` along +v at −u, leg of width `b` along +u at −v.
//! - `T`: flange at +v, web towards −v.
//! - `Z`: web on v; top flange towards −u, bottom flange towards +u; `b` is each flange's width
//!   including the web.
//! - Sloped flanges (`I`, `U`): `slope` is the angle of the inner flange face (rad), which is
//!   thinner towards the toe. `tf` is the thickness halfway along the flange width: at
//!   |u| = b/4 for `I` and at u = 0 for `U` (b/2 from the toe).
//! - Radii: `I`, `AsymI`, `T`, `Z` `r` and `U`, `L` `r1` are root fillets; `U`, `L` `r2` round
//!   the inner edge of the toes; `C` `r` is the inner bend radius (outer `r + t`); `Rhs` has
//!   outer and inner corner radii.

use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::sync::OnceLock;

use glam::{DAffine2, DVec2};

use crate::model::{ImportedProps, PropsSource, SectionProps, Shape};

/// Default chord tolerance (m) for tessellating arcs.
pub const CHORD_TOL: f64 = 1e-4;
/// Default maximum segments per quarter circle.
pub const MAX_PER_QUARTER: u32 = 8;

/// One boundary edge, starting where the previous one ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edge {
    Line { to: DVec2 },
    /// A circular arc about `centre`, counter-clockwise when `ccw`.
    Arc { centre: DVec2, to: DVec2, ccw: bool },
}

impl Edge {
    pub fn end(&self) -> DVec2 {
        match *self {
            Edge::Line { to } | Edge::Arc { to, .. } => to,
        }
    }
}

/// A closed boundary loop of lines and arcs.
#[derive(Clone, Debug, PartialEq)]
pub struct Contour {
    pub start: DVec2,
    /// The last edge ends at `start`.
    pub edges: Vec<Edge>,
}

/// How the shear centre of an outline is known.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShearCentre {
    /// Doubly symmetric or point-symmetric: the centroid.
    Centroid,
    /// A closed form for the shape.
    At(DVec2),
    /// Not known from the shape alone.
    Unknown,
}

/// A section outline: outer loops counter-clockwise, holes clockwise.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline2 {
    pub outer: Vec<Contour>,
    pub holes: Vec<Contour>,
    pub shear_centre: ShearCentre,
    /// Chord tolerance (m) for [`Outline2::loops`].
    pub chord_tol: f64,
    /// Maximum segments per quarter circle for [`Outline2::loops`].
    pub max_per_quarter: u32,
}

/// A tessellated loop.
#[derive(Clone, Debug, PartialEq)]
pub struct Loop {
    /// Without the closing repeat of the first point.
    pub points: Vec<DVec2>,
    pub hole: bool,
}

/// Why a shape has no outline.
#[derive(Clone, Debug, PartialEq)]
pub enum ShapeError {
    Unknown,
    Invalid(String),
}

impl std::fmt::Display for ShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShapeError::Unknown => f.write_str("unknown shape"),
            ShapeError::Invalid(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ShapeError {}

// ---------------------------------------------------------------------------------------------
// Contours

fn cross(a: DVec2, b: DVec2) -> f64 {
    a.x * b.y - a.y * b.x
}

/// Signed sweep angle of an arc from `from` to `to` about `centre`, in (0, 2π] for ccw and
/// [−2π, 0) for cw.
fn sweep(centre: DVec2, from: DVec2, to: DVec2, ccw: bool) -> f64 {
    let a0 = (from - centre).to_angle();
    let a1 = (to - centre).to_angle();
    if ccw {
        let mut s = (a1 - a0).rem_euclid(TAU);
        if s <= 1e-15 {
            s = TAU;
        }
        s
    } else {
        let mut s = (a0 - a1).rem_euclid(TAU);
        if s <= 1e-15 {
            s = TAU;
        }
        -s
    }
}

impl Contour {
    /// A polygon contour.
    pub fn polygon(points: &[DVec2]) -> Contour {
        Contour {
            start: points[0],
            edges: points[1..].iter().chain(std::iter::once(&points[0])).map(|&to| Edge::Line { to }).collect(),
        }
    }

    /// A full circle as four counter-clockwise quarter arcs.
    pub fn circle(centre: DVec2, r: f64) -> Contour {
        let p = |a: f64| centre + DVec2::from_angle(a) * r;
        Contour {
            start: p(0.0),
            edges: (1..=4).map(|k| Edge::Arc { centre, to: p(k as f64 * FRAC_PI_2), ccw: true }).collect(),
        }
    }

    /// A polygon given by corners with a fillet radius each, as tangent arcs (0 = sharp).
    /// Left turns make convex roundings, right turns concave fillets.
    pub fn rounded(corners: &[(DVec2, f64)]) -> Contour {
        let n = corners.len();
        // Entry point, exit point and arc (centre, ccw) per corner.
        let mut parts: Vec<(DVec2, DVec2, Option<(DVec2, bool)>)> = Vec::with_capacity(n);
        for i in 0..n {
            let (p, r) = corners[i];
            let prev = corners[(i + n - 1) % n].0;
            let next = corners[(i + 1) % n].0;
            let d1 = (p - prev).normalize();
            let d2 = (next - p).normalize();
            let turn = cross(d1, d2);
            if r <= 0.0 || turn.abs() < 1e-15 {
                parts.push((p, p, None));
                continue;
            }
            let delta = d1.dot(d2).clamp(-1.0, 1.0).acos();
            let t = r * (delta / 2.0).tan();
            let t1 = p - d1 * t;
            let t2 = p + d2 * t;
            let left = DVec2::new(-d1.y, d1.x);
            let ccw = turn > 0.0;
            let centre = if ccw { t1 + left * r } else { t1 - left * r };
            parts.push((t1, t2, Some((centre, ccw))));
        }
        let start = parts[0].1;
        let mut edges = Vec::new();
        let mut at = start;
        for k in 1..=n {
            let (entry, exit, arc) = parts[k % n];
            if (entry - at).length() > 1e-15 {
                edges.push(Edge::Line { to: entry });
            }
            if let Some((centre, ccw)) = arc {
                edges.push(Edge::Arc { centre, to: exit, ccw });
            }
            at = exit;
        }
        Contour { start, edges }
    }

    /// The same loop traversed the other way.
    pub fn reversed(&self) -> Contour {
        let mut starts = Vec::with_capacity(self.edges.len());
        let mut at = self.start;
        for e in &self.edges {
            starts.push(at);
            at = e.end();
        }
        let edges = self
            .edges
            .iter()
            .zip(starts)
            .rev()
            .map(|(e, from)| match *e {
                Edge::Line { .. } => Edge::Line { to: from },
                Edge::Arc { centre, ccw, .. } => Edge::Arc { centre, to: from, ccw: !ccw },
            })
            .collect();
        Contour { start: self.start, edges }
    }

    /// Applies an affine transform. Arcs stay arcs under similarity transforms; otherwise they
    /// are replaced by fine chords (1e-7 m).
    pub fn transformed(&self, t: &DAffine2) -> Contour {
        let m = t.matrix2;
        let det = m.determinant();
        let c0 = m.x_axis;
        let c1 = m.y_axis;
        let similar = (c0.length() - c1.length()).abs() <= 1e-12 * c0.length().max(1.0) && c0.dot(c1).abs() <= 1e-12 * c0.length_squared().max(1.0);
        let mut edges = Vec::new();
        let mut at = self.start;
        for e in &self.edges {
            match *e {
                Edge::Line { to } => edges.push(Edge::Line { to: t.transform_point2(to) }),
                Edge::Arc { centre, to, ccw } if similar => edges.push(Edge::Arc {
                    centre: t.transform_point2(centre),
                    to: t.transform_point2(to),
                    ccw: if det < 0.0 { !ccw } else { ccw },
                }),
                Edge::Arc { .. } => {
                    for p in tessellate_edge(at, e, 1e-7, 1 << 12).into_iter().skip(1) {
                        edges.push(Edge::Line { to: t.transform_point2(p) });
                    }
                    edges.push(Edge::Line { to: t.transform_point2(e.end()) });
                }
            }
            at = e.end();
        }
        Contour { start: t.transform_point2(self.start), edges }
    }

    /// Calls `f(from, edge)` for every edge.
    fn walk(&self, mut f: impl FnMut(DVec2, &Edge)) {
        let mut at = self.start;
        for e in &self.edges {
            f(at, e);
            at = e.end();
        }
    }

    /// Signed area (positive counter-clockwise).
    pub fn signed_area(&self) -> f64 {
        let mut i = Integrals::default();
        self.walk(|from, e| i.add_edge(from, e, DVec2::ZERO));
        i.a
    }

    /// Tessellated points, without the closing repeat.
    pub fn points(&self, chord_tol: f64, max_per_quarter: u32) -> Vec<DVec2> {
        let mut out = vec![self.start];
        self.walk(|from, e| {
            let pts = tessellate_edge(from, e, chord_tol, max_per_quarter);
            out.extend(pts.into_iter().skip(1));
            out.push(e.end());
        });
        out.pop();
        out
    }

    fn bbox(&self, lo: &mut DVec2, hi: &mut DVec2) {
        let mut grow = |p: DVec2| {
            *lo = lo.min(p);
            *hi = hi.max(p);
        };
        grow(self.start);
        self.walk(|from, e| {
            grow(e.end());
            if let Edge::Arc { centre, to, ccw } = *e {
                let r = (from - centre).length();
                let a0 = (from - centre).to_angle();
                let s = sweep(centre, from, to, ccw);
                for k in 0..4 {
                    let a = k as f64 * FRAC_PI_2;
                    let rel = if s > 0.0 { (a - a0).rem_euclid(TAU) } else { (a0 - a).rem_euclid(TAU) };
                    if rel <= s.abs() {
                        grow(centre + DVec2::from_angle(a) * r);
                    }
                }
            }
        });
    }
}

/// Points along an edge from `from`, without the end point.
fn tessellate_edge(from: DVec2, e: &Edge, chord_tol: f64, max_per_quarter: u32) -> Vec<DVec2> {
    match *e {
        Edge::Line { .. } => vec![from],
        Edge::Arc { centre, to, ccw } => {
            let r = (from - centre).length();
            let s = sweep(centre, from, to, ccw);
            let step = if chord_tol > 0.0 && chord_tol < r { 2.0 * (1.0 - chord_tol / r).acos() } else { PI };
            let by_tol = (s.abs() / step).ceil();
            let by_cap = (max_per_quarter.max(1) as f64 * s.abs() / FRAC_PI_2).ceil();
            let n = by_tol.min(by_cap).max(1.0) as usize;
            let a0 = (from - centre).to_angle();
            (0..n).map(|k| if k == 0 { from } else { centre + DVec2::from_angle(a0 + s * k as f64 / n as f64) * r }).collect()
        }
    }
}

impl Outline2 {
    /// Tessellated loops: outer loops counter-clockwise, then holes clockwise.
    pub fn loops(&self) -> Vec<Loop> {
        let tess = |c: &Contour, hole| Loop { points: c.points(self.chord_tol, self.max_per_quarter), hole };
        self.outer.iter().map(|c| tess(c, false)).chain(self.holes.iter().map(|c| tess(c, true))).collect()
    }

    /// `[min, max]` of the exact outline.
    pub fn bbox(&self) -> [DVec2; 2] {
        let mut lo = DVec2::splat(f64::INFINITY);
        let mut hi = DVec2::splat(f64::NEG_INFINITY);
        for c in self.outer.iter().chain(&self.holes) {
            c.bbox(&mut lo, &mut hi);
        }
        [lo, hi]
    }
}

// ---------------------------------------------------------------------------------------------
// Outlines of shapes

fn invalid(msg: impl Into<String>) -> ShapeError {
    ShapeError::Invalid(msg.into())
}

/// Checks dimensions; the message names what is wrong.
pub fn check(shape: &Shape) -> Result<(), ShapeError> {
    let pos = |name: &str, v: f64| if v > 0.0 && v.is_finite() { Ok(()) } else { Err(invalid(format!("{name} must be positive, got {v}"))) };
    let nonneg = |name: &str, v: f64| if v >= 0.0 && v.is_finite() { Ok(()) } else { Err(invalid(format!("{name} must not be negative, got {v}"))) };
    let less = |what: &str, a: f64, b: f64| if a < b { Ok(()) } else { Err(invalid(format!("{what}: {a} is not below {b}"))) };
    let slope_ok = |s: f64| if s.is_finite() && s.abs() < FRAC_PI_2 * 0.5 { Ok(()) } else { Err(invalid(format!("flange slope {s} rad out of range"))) };
    match *shape {
        Shape::I { h, b, tw, tf, r, slope } => {
            pos("h", h)?; pos("b", b)?; pos("tw", tw)?; pos("tf", tf)?; nonneg("r", r)?; slope_ok(slope)?;
            less("tw vs b", tw, b)?;
            let s = slope.tan();
            let toe = tf - b / 4.0 * s;
            let root = tf + (b / 4.0 - tw / 2.0) * s;
            pos("flange thickness at the toe", toe)?;
            less("2 * root flange thickness + 2 r vs h", 2.0 * (root + r), h)?;
            less("tw / 2 + r vs b / 2", tw / 2.0 + r, b / 2.0)
        }
        Shape::AsymI { h, bt, bb, tw, tft, tfb, r } => {
            pos("h", h)?; pos("bt", bt)?; pos("bb", bb)?; pos("tw", tw)?; pos("tft", tft)?; pos("tfb", tfb)?; nonneg("r", r)?;
            less("tw + 2 r vs bt", tw + 2.0 * r, bt)?;
            less("tw + 2 r vs bb", tw + 2.0 * r, bb)?;
            less("tft + tfb + 2 r vs h", tft + tfb + 2.0 * r, h)
        }
        Shape::U { h, b, tw, tf, r1, r2, slope } => {
            pos("h", h)?; pos("b", b)?; pos("tw", tw)?; pos("tf", tf)?; nonneg("r1", r1)?; nonneg("r2", r2)?; slope_ok(slope)?;
            let s = slope.tan();
            let toe = tf - b / 2.0 * s;
            let root = tf + (b / 2.0 - tw) * s;
            pos("flange thickness at the toe", toe - r2)?;
            less("tw + r1 + r2 vs b", tw + r1 + r2, b)?;
            less("2 * root flange thickness + 2 r1 vs h", 2.0 * (root + r1), h)
        }
        Shape::L { h, b, t, r1, r2 } => {
            pos("h", h)?; pos("b", b)?; pos("t", t)?; nonneg("r1", r1)?; nonneg("r2", r2)?;
            less("t + r1 + r2 vs b", t + r1 + r2, b)?;
            less("t + r1 + r2 vs h", t + r1 + r2, h)?;
            less("r2 vs t", r2, t)
        }
        Shape::T { h, b, tw, tf, r } => {
            pos("h", h)?; pos("b", b)?; pos("tw", tw)?; pos("tf", tf)?; nonneg("r", r)?;
            less("tw + 2 r vs b", tw + 2.0 * r, b)?;
            less("tf + r vs h", tf + r, h)
        }
        Shape::C { h, b, t, lip, r } => {
            pos("h", h)?; pos("b", b)?; pos("t", t)?; nonneg("lip", lip)?; nonneg("r", r)?;
            less("2 (t + r) vs b", 2.0 * (t + r), b + 1e-15)?;
            if lip > 0.0 {
                if lip < t + r {
                    return Err(invalid(format!("lip {lip} shorter than t + r")));
                }
                less("2 lip vs h", 2.0 * lip, h)
            } else {
                less("2 (t + r) vs h", 2.0 * (t + r), h)
            }
        }
        Shape::Z { h, b, tw, tf, r } => {
            pos("h", h)?; pos("b", b)?; pos("tw", tw)?; pos("tf", tf)?; nonneg("r", r)?;
            less("tw + r vs b", tw + r, b)?;
            less("2 (tf + r) vs h", 2.0 * (tf + r), h)
        }
        Shape::Rect { b, h } => {
            pos("b", b)?;
            pos("h", h)
        }
        Shape::Circle { d } => pos("d", d),
        Shape::Chs { d, t } => {
            pos("d", d)?; pos("t", t)?;
            less("2 t vs d", 2.0 * t, d)
        }
        Shape::Rhs { b, h, t, r_out, r_in } => {
            pos("b", b)?; pos("h", h)?; pos("t", t)?; nonneg("r_out", r_out)?; nonneg("r_in", r_in)?;
            less("2 t vs b", 2.0 * t, b)?;
            less("2 t vs h", 2.0 * t, h)?;
            less("2 r_out vs min(b, h)", 2.0 * r_out, b.min(h) + 1e-15)?;
            less("2 r_in vs min(b, h) - 2 t", 2.0 * r_in, b.min(h) - 2.0 * t + 1e-15)
        }
        Shape::Polygon { ref outer, ref holes } => {
            if outer.len() < 3 {
                return Err(invalid("polygon outer loop needs at least 3 points"));
            }
            if holes.iter().any(|h| h.len() < 3) {
                return Err(invalid("polygon hole needs at least 3 points"));
            }
            if Contour::polygon(outer).signed_area().abs() <= 0.0 {
                return Err(invalid("polygon outer loop has zero area"));
            }
            Ok(())
        }
        Shape::Composite(ref parts) => {
            if parts.is_empty() {
                return Err(invalid("composite without parts"));
            }
            for (s, t) in parts {
                if t.matrix2.determinant().abs() < 1e-12 {
                    return Err(invalid("composite transform is singular"));
                }
                check(s)?;
            }
            Ok(())
        }
        Shape::Unknown => Err(ShapeError::Unknown),
    }
}

fn v(u: f64, v: f64) -> DVec2 {
    DVec2::new(u, v)
}

/// The outline of a shape (see the module documentation for each shape's orientation).
pub fn outline(shape: &Shape, chord_tol: f64, max_per_quarter: u32) -> Result<Outline2, ShapeError> {
    check(shape)?;
    let mut out = Outline2 { outer: Vec::new(), holes: Vec::new(), shear_centre: ShearCentre::Centroid, chord_tol, max_per_quarter };
    match *shape {
        Shape::I { h, b, tw, tf, r, slope } => {
            let s = slope.tan();
            let (hh, hb, hw) = (h / 2.0, b / 2.0, tw / 2.0);
            let toe = hh - tf + b / 4.0 * s; // |v| of the inner flange face at the toe
            let root = hh - tf - (b / 4.0 - hw) * s; // ... at the web
            out.outer.push(Contour::rounded(&[
                (v(-hb, -hh), 0.0),
                (v(hb, -hh), 0.0),
                (v(hb, -toe), 0.0),
                (v(hw, -root), r),
                (v(hw, root), r),
                (v(hb, toe), 0.0),
                (v(hb, hh), 0.0),
                (v(-hb, hh), 0.0),
                (v(-hb, toe), 0.0),
                (v(-hw, root), r),
                (v(-hw, -root), r),
                (v(-hb, -toe), 0.0),
            ]));
        }
        Shape::AsymI { h, bt, bb, tw, tft, tfb, r } => {
            let (hh, hw) = (h / 2.0, tw / 2.0);
            out.outer.push(Contour::rounded(&[
                (v(-bb / 2.0, -hh), 0.0),
                (v(bb / 2.0, -hh), 0.0),
                (v(bb / 2.0, -hh + tfb), 0.0),
                (v(hw, -hh + tfb), r),
                (v(hw, hh - tft), r),
                (v(bt / 2.0, hh - tft), 0.0),
                (v(bt / 2.0, hh), 0.0),
                (v(-bt / 2.0, hh), 0.0),
                (v(-bt / 2.0, hh - tft), 0.0),
                (v(-hw, hh - tft), r),
                (v(-hw, -hh + tfb), r),
                (v(-bb / 2.0, -hh + tfb), 0.0),
            ]));
            // Thin-walled monosymmetric I: the flanges share the shear in proportion to their
            // own second moments.
            let it = tft * bt.powi(3);
            let ib = tfb * bb.powi(3);
            let vt = hh - tft / 2.0;
            let vb = -hh + tfb / 2.0;
            out.shear_centre = ShearCentre::At(v(0.0, (it * vt + ib * vb) / (it + ib)));
        }
        Shape::U { h, b, tw, tf, r1, r2, slope } => {
            let s = slope.tan();
            let (hh, hb) = (h / 2.0, b / 2.0);
            let toe = hh - tf + hb * s;
            let root = hh - tf - (hb - tw) * s;
            let web = -hb + tw;
            out.outer.push(Contour::rounded(&[
                (v(-hb, -hh), 0.0),
                (v(hb, -hh), 0.0),
                (v(hb, -toe), r2),
                (v(web, -root), r1),
                (v(web, root), r1),
                (v(hb, toe), r2),
                (v(hb, hh), 0.0),
                (v(-hb, hh), 0.0),
            ]));
            // Centreline dimensions; e from the web centreline, away from the flanges.
            let bp = b - tw / 2.0;
            let hp = h - tf;
            let e = 3.0 * bp * bp * tf / (6.0 * bp * tf + hp * tw);
            out.shear_centre = ShearCentre::At(v(-hb + tw / 2.0 - e, 0.0));
        }
        Shape::L { h, b, t, r1, r2 } => {
            let (hh, hb) = (h / 2.0, b / 2.0);
            out.outer.push(Contour::rounded(&[
                (v(-hb, -hh), 0.0),
                (v(hb, -hh), 0.0),
                (v(hb, -hh + t), r2),
                (v(-hb + t, -hh + t), r1),
                (v(-hb + t, hh), r2),
                (v(-hb, hh), 0.0),
            ]));
            out.shear_centre = ShearCentre::At(v(-hb + t / 2.0, -hh + t / 2.0));
        }
        Shape::T { h, b, tw, tf, r } => {
            let (hh, hb, hw) = (h / 2.0, b / 2.0, tw / 2.0);
            out.outer.push(Contour::rounded(&[
                (v(-hw, -hh), 0.0),
                (v(hw, -hh), 0.0),
                (v(hw, hh - tf), r),
                (v(hb, hh - tf), 0.0),
                (v(hb, hh), 0.0),
                (v(-hb, hh), 0.0),
                (v(-hb, hh - tf), 0.0),
                (v(-hw, hh - tf), r),
            ]));
            out.shear_centre = ShearCentre::At(v(0.0, hh - tf / 2.0));
        }
        Shape::C { h, b, t, lip, r } => {
            let (hh, hb) = (h / 2.0, b / 2.0);
            let ro = r + t;
            let corners: Vec<(DVec2, f64)> = if lip > 0.0 {
                vec![
                    (v(-hb, -hh), ro),
                    (v(hb, -hh), ro),
                    (v(hb, -hh + lip), 0.0),
                    (v(hb - t, -hh + lip), 0.0),
                    (v(hb - t, -hh + t), r),
                    (v(-hb + t, -hh + t), r),
                    (v(-hb + t, hh - t), r),
                    (v(hb - t, hh - t), r),
                    (v(hb - t, hh - lip), 0.0),
                    (v(hb, hh - lip), 0.0),
                    (v(hb, hh), ro),
                    (v(-hb, hh), ro),
                ]
            } else {
                vec![
                    (v(-hb, -hh), ro),
                    (v(hb, -hh), 0.0),
                    (v(hb, -hh + t), 0.0),
                    (v(-hb + t, -hh + t), r),
                    (v(-hb + t, hh - t), r),
                    (v(hb, hh - t), 0.0),
                    (v(hb, hh), 0.0),
                    (v(-hb, hh), ro),
                ]
            };
            out.outer.push(Contour::rounded(&corners));
            // Lipped channel, centreline dimensions a (web), w (flange), c (lip). With a lip the
            // flange centreline ends on the lip centreline (b − t); without one it runs to the
            // free tip (b − t/2), and m reduces to the U closed form with tf = tw = t.
            let a = h - t;
            let w = if lip > 0.0 { b - t } else { b - t / 2.0 };
            let c = if lip > 0.0 { lip - t / 2.0 } else { 0.0 };
            let m = w * (3.0 * a * a * w + c * (6.0 * a * a - 8.0 * c * c))
                / (a.powi(3) + 6.0 * a * a * w + c * (8.0 * c * c - 12.0 * a * c + 6.0 * a * a));
            out.shear_centre = ShearCentre::At(v(-hb + t / 2.0 - m, 0.0));
        }
        Shape::Z { h, b, tw, tf, r } => {
            let (hh, hw) = (h / 2.0, tw / 2.0);
            let reach = b - hw;
            out.outer.push(Contour::rounded(&[
                (v(-hw, -hh), 0.0),
                (v(reach, -hh), 0.0),
                (v(reach, -hh + tf), 0.0),
                (v(hw, -hh + tf), r),
                (v(hw, hh), 0.0),
                (v(-reach, hh), 0.0),
                (v(-reach, hh - tf), 0.0),
                (v(-hw, hh - tf), r),
            ]));
        }
        Shape::Rect { b, h } => {
            out.outer.push(Contour::polygon(&[v(-b / 2.0, -h / 2.0), v(b / 2.0, -h / 2.0), v(b / 2.0, h / 2.0), v(-b / 2.0, h / 2.0)]));
        }
        Shape::Circle { d } => out.outer.push(Contour::circle(DVec2::ZERO, d / 2.0)),
        Shape::Chs { d, t } => {
            out.outer.push(Contour::circle(DVec2::ZERO, d / 2.0));
            out.holes.push(Contour::circle(DVec2::ZERO, d / 2.0 - t).reversed());
        }
        Shape::Rhs { b, h, t, r_out, r_in } => {
            let rect = |b: f64, h: f64, r: f64| {
                Contour::rounded(&[(v(-b / 2.0, -h / 2.0), r), (v(b / 2.0, -h / 2.0), r), (v(b / 2.0, h / 2.0), r), (v(-b / 2.0, h / 2.0), r)])
            };
            out.outer.push(rect(b, h, r_out));
            out.holes.push(rect(b - 2.0 * t, h - 2.0 * t, r_in).reversed());
        }
        Shape::Polygon { ref outer, ref holes } => {
            let c = Contour::polygon(outer);
            out.outer.push(if c.signed_area() < 0.0 { c.reversed() } else { c });
            for h in holes {
                let c = Contour::polygon(h);
                out.holes.push(if c.signed_area() > 0.0 { c.reversed() } else { c });
            }
            out.shear_centre = ShearCentre::Unknown;
        }
        Shape::Composite(ref parts) => {
            for (s, t) in parts {
                let part = outline(s, chord_tol, max_per_quarter)?;
                for c in &part.outer {
                    let c = c.transformed(t);
                    out.outer.push(if c.signed_area() < 0.0 { c.reversed() } else { c });
                }
                for c in &part.holes {
                    let c = c.transformed(t);
                    out.holes.push(if c.signed_area() > 0.0 { c.reversed() } else { c });
                }
            }
            out.shear_centre = ShearCentre::Unknown;
        }
        Shape::Unknown => return Err(ShapeError::Unknown),
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Properties

/// Gauss-Legendre nodes and weights on [−1, 1].
fn gauss() -> &'static (Vec<f64>, Vec<f64>) {
    static G: OnceLock<(Vec<f64>, Vec<f64>)> = OnceLock::new();
    G.get_or_init(|| {
        const N: usize = 16;
        let mut x = vec![0.0; N];
        let mut w = vec![0.0; N];
        for i in 0..N {
            let mut z = (PI * (i as f64 + 0.75) / (N as f64 + 0.5)).cos();
            for _ in 0..100 {
                let (mut p0, mut p1) = (1.0, z);
                for k in 2..=N {
                    let p2 = ((2 * k - 1) as f64 * z * p1 - (k - 1) as f64 * p0) / k as f64;
                    p0 = p1;
                    p1 = p2;
                }
                let dp = N as f64 * (z * p1 - p0) / (z * z - 1.0);
                let dz = p1 / dp;
                z -= dz;
                if dz.abs() < 1e-16 {
                    let (mut p0, mut p1) = (1.0, z);
                    for k in 2..=N {
                        let p2 = ((2 * k - 1) as f64 * z * p1 - (k - 1) as f64 * p0) / k as f64;
                        p0 = p1;
                        p1 = p2;
                    }
                    let dp = N as f64 * (z * p1 - p0) / (z * z - 1.0);
                    w[i] = 2.0 / ((1.0 - z * z) * dp * dp);
                    break;
                }
            }
            x[i] = z;
        }
        (x, w)
    })
}

/// Area integrals `∮ g(u, v) dv` (Green's theorem) about a reference point.
#[derive(Default, Clone, Copy, Debug)]
struct Integrals {
    a: f64,
    su: f64,
    sv: f64,
    uu: f64,
    vv: f64,
    uv: f64,
}

impl Integrals {
    fn sample(&mut self, p: DVec2, dv: f64) {
        let (u, w) = (p.x, p.y);
        self.a += u * dv;
        self.su += 0.5 * u * u * dv;
        self.sv += u * w * dv;
        self.uu += u * u * u / 3.0 * dv;
        self.vv += u * w * w * dv;
        self.uv += 0.5 * u * u * w * dv;
    }

    fn add_edge(&mut self, from: DVec2, e: &Edge, rel: DVec2) {
        let (x, w) = gauss();
        match *e {
            Edge::Line { to } => {
                let (a, b) = (from - rel, to - rel);
                let d = b - a;
                for (xi, wi) in x.iter().zip(w) {
                    let t = 0.5 * (xi + 1.0);
                    self.sample(a + d * t, 0.5 * wi * d.y);
                }
            }
            Edge::Arc { centre, to, ccw } => {
                let r = (from - centre).length();
                let s = sweep(centre, from, to, ccw);
                let a0 = (from - centre).to_angle();
                let c = centre - rel;
                // Pieces of at most a quarter turn keep the quadrature at rounding level.
                let pieces = (s.abs() / FRAC_PI_2).ceil().max(1.0) as usize;
                let ds = s / pieces as f64;
                for k in 0..pieces {
                    let t0 = a0 + ds * k as f64;
                    for (xi, wi) in x.iter().zip(w) {
                        let th = t0 + 0.5 * (xi + 1.0) * ds;
                        let p = c + DVec2::new(th.cos(), th.sin()) * r;
                        // dv = r cos θ dθ
                        self.sample(p, 0.5 * wi * ds * r * th.cos());
                    }
                }
            }
        }
    }
}

/// Exact area, centroid and second moments of an outline; the shear centre from
/// [`Outline2::shear_centre`] (the centroid when unknown; see [`compute`] for the source flag).
/// The torsion constant is computed only for solid and hollow circles.
pub fn props(outline: &Outline2) -> SectionProps {
    let bbox = outline.bbox();
    let rel = (bbox[0] + bbox[1]) * 0.5;
    let mut i = Integrals::default();
    for c in outline.outer.iter().chain(&outline.holes) {
        c.walk(|from, e| i.add_edge(from, e, rel));
    }
    let cu = i.su / i.a;
    let cv = i.sv / i.a;
    let centroid = rel + DVec2::new(cu, cv);
    let shear_centre = match outline.shear_centre {
        ShearCentre::At(p) => p,
        ShearCentre::Centroid | ShearCentre::Unknown => centroid,
    };
    SectionProps {
        area: i.a,
        centroid,
        shear_centre,
        iy: i.vv - i.a * cv * cv,
        iz: i.uu - i.a * cu * cu,
        iyz: i.uv - i.a * cu * cv,
        it: None,
        bbox,
    }
}

/// Whether every property is a finite number and the area positive (false when the
/// dimensions are beyond what f64 integrals can hold).
pub fn props_finite(p: &SectionProps) -> bool {
    let scalars = [p.area, p.iy, p.iz, p.iyz, p.it.unwrap_or(0.0)];
    let points = [p.centroid, p.shear_centre, p.bbox[0], p.bbox[1]];
    p.area > 0.0 && scalars.iter().all(|x| x.is_finite()) && points.iter().all(|q| q.is_finite())
}

/// Outline and properties of a shape, with the shear centre from the imported properties when
/// the shape gives none. `Unavailable` (with default properties) when the shape has no outline
/// or its properties are not finite.
pub fn compute(shape: &Shape, imported: Option<&ImportedProps>) -> (SectionProps, PropsSource) {
    let outline = match outline(shape, CHORD_TOL, MAX_PER_QUARTER) {
        Ok(o) => o,
        Err(_) => return (SectionProps::default(), PropsSource::Unavailable),
    };
    let mut p = props(&outline);
    p.it = match *shape {
        Shape::Circle { d } => Some(PI * d.powi(4) / 32.0),
        Shape::Chs { d, t } => Some(PI * (d.powi(4) - (d - 2.0 * t).powi(4)) / 32.0),
        _ => None,
    };
    if !props_finite(&p) {
        return (SectionProps::default(), PropsSource::Unavailable);
    }
    let source = match outline.shear_centre {
        ShearCentre::Centroid | ShearCentre::At(_) => PropsSource::Computed,
        ShearCentre::Unknown => match imported.and_then(|i| i.shear_centre) {
            Some(sc) => {
                p.shear_centre = sc;
                PropsSource::Imported
            }
            None => PropsSource::ShearCentreApproximate,
        },
    };
    (p, source)
}

// ---------------------------------------------------------------------------------------------
// Cardinal points

/// The anchor of IFC4 `IfcCardinalPointReference` `cp` in profile (u, v).
///
/// 1-9 are points of the bounding box (bottom row 1-3, mid-depth 4-6, top 7-9; within a row
/// left, centre, right). Seen looking along the extrusion direction, *left* is +u and *top* is
/// +v, so 1 is (u_max, v_min) and 9 is (u_min, v_max). 10 is the centroid, 11-14 the centroid
/// projected onto the bottom, left, right and top edges, 15 the shear centre and 16-19 the shear
/// centre projected likewise. 0 means no anchor (zero offset). Any other value gives `None`; the
/// caller adds a diagnostic and treats it as 0.
pub fn cardinal_anchor(props: &SectionProps, cp: u8) -> Option<DVec2> {
    let [lo, hi] = props.bbox;
    let mid = (lo + hi) * 0.5;
    let (left, centre, right) = (hi.x, mid.x, lo.x);
    let (bottom, middle, top) = (lo.y, mid.y, hi.y);
    let projected = |p: DVec2, k: u8| match k {
        0 => v(p.x, bottom),
        1 => v(left, p.y),
        2 => v(right, p.y),
        _ => v(p.x, top),
    };
    Some(match cp {
        0 => DVec2::ZERO,
        1 => v(left, bottom),
        2 => v(centre, bottom),
        3 => v(right, bottom),
        4 => v(left, middle),
        5 => v(centre, middle),
        6 => v(right, middle),
        7 => v(left, top),
        8 => v(centre, top),
        9 => v(right, top),
        10 => props.centroid,
        11..=14 => projected(props.centroid, cp - 11),
        15 => props.shear_centre,
        16..=19 => projected(props.shear_centre, cp - 16),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(a: f64, b: f64) -> f64 {
        ((a - b) / b).abs()
    }

    #[test]
    fn gauss_weights_sum_to_two() {
        let (x, w) = gauss();
        assert!((w.iter().sum::<f64>() - 2.0).abs() < 1e-14);
        // x^14 integrates exactly.
        let s: f64 = x.iter().zip(w).map(|(x, w)| w * x.powi(14)).sum();
        assert!((s - 2.0 / 15.0).abs() < 1e-14);
    }

    #[test]
    fn outer_ccw_holes_cw() {
        let shapes = [
            Shape::I { h: 0.2, b: 0.1, tw: 0.0056, tf: 0.0085, r: 0.012, slope: 0.0 },
            Shape::I { h: 0.2, b: 0.09, tw: 0.0075, tf: 0.0113, r: 0.0075, slope: 0.14f64.atan() },
            Shape::AsymI { h: 0.3, bt: 0.12, bb: 0.2, tw: 0.008, tft: 0.01, tfb: 0.015, r: 0.01 },
            Shape::U { h: 0.2, b: 0.075, tw: 0.0085, tf: 0.0115, r1: 0.0115, r2: 0.006, slope: 0.08f64.atan() },
            Shape::L { h: 0.1, b: 0.1, t: 0.01, r1: 0.012, r2: 0.006 },
            Shape::T { h: 0.1, b: 0.1, tw: 0.011, tf: 0.011, r: 0.011 },
            Shape::C { h: 0.2, b: 0.07, t: 0.002, lip: 0.02, r: 0.003 },
            Shape::C { h: 0.2, b: 0.07, t: 0.002, lip: 0.0, r: 0.003 },
            Shape::Z { h: 0.2, b: 0.07, tw: 0.002, tf: 0.002, r: 0.003 },
            Shape::Rect { b: 0.1, h: 0.3 },
            Shape::Circle { d: 0.1 },
            Shape::Chs { d: 0.1, t: 0.005 },
            Shape::Rhs { b: 0.1, h: 0.2, t: 0.006, r_out: 0.012, r_in: 0.006 },
        ];
        for s in &shapes {
            let o = outline(s, CHORD_TOL, MAX_PER_QUARTER).unwrap();
            for c in &o.outer {
                assert!(c.signed_area() > 0.0, "{s:?}");
            }
            for c in &o.holes {
                assert!(c.signed_area() < 0.0, "{s:?}");
            }
            // Closed contours: the last edge ends at the start.
            for c in o.outer.iter().chain(&o.holes) {
                assert!((c.edges.last().unwrap().end() - c.start).length() < 1e-15);
            }
            // Bounding box centred on the origin for every parametric shape but Z... and Z too.
            let [lo, hi] = o.bbox();
            assert!((lo + hi).length() < 1e-15, "{s:?} {lo} {hi}");
            // The tessellation stays within the chord tolerance of the exact area.
            let p = props(&o);
            let tess: f64 = o
                .loops()
                .iter()
                .map(|l| Contour::polygon(&l.points).signed_area())
                .sum();
            assert!(rel(tess, p.area) < 1e-2, "{s:?}");
        }
    }

    #[test]
    fn transformed_composite_matches_parts() {
        let rect = Shape::Rect { b: 0.2, h: 0.01 };
        let mirror = DAffine2::from_cols_array(&[-1.0, 0.0, 0.0, 1.0, 0.0, 0.1]);
        let comp = Shape::Composite(vec![
            (rect.clone(), DAffine2::from_translation(v(0.0, -0.1))),
            (Shape::Circle { d: 0.05 }, mirror),
        ]);
        let p = props(&outline(&comp, CHORD_TOL, MAX_PER_QUARTER).unwrap());
        let a = 0.2 * 0.01 + PI * 0.025 * 0.025;
        assert!(rel(p.area, a) < 1e-12);
        assert!((p.centroid.y - (-0.1 * 0.002 + 0.1 * PI * 0.025 * 0.025) / a).abs() < 1e-12);
    }
}
