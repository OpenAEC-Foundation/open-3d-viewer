//! A shape as FreeCAD saves it (`*.brp`): the text form of a boundary representation, its faces
//! triangulated here.
//!
//! The file lists, each numbered from 1: locations (a 3 × 4 matrix, or a product of powers of
//! others), 2D curves (in a surface's u, v), 3D curves, surfaces, and the shapes, innermost first:
//! vertices, edges (their 3D curve and, per face they bound, their curve in its surface), wires
//! (edges in order), faces (a surface and its wires), shells, solids and compounds; a shape names
//! its parts counted back from the end, each with a sense (`+`, `-`) and a location. The last
//! line names the root.
//!
//! A face is triangulated in its surface's (u, v): its wires become polygons there, from its
//! edges' curves in the surface (on a plane, where those may be absent, from their 3D curves),
//! the polygons are cut into triangles, and on a curved surface the triangles are split until
//! none spans more than a step of it. Points and normals are the surface's.

use crate::model::Layer;
use glam::{DMat4, DVec3};
use std::collections::HashMap;
use std::f64::consts::TAU;

/// The turn a curved surface or a circle is cut in.
const STEP: f64 = TAU / 32.0;

/// The part of a polygon on one side of the line where coordinate `axis` is `at`: at or beyond
/// it (`beyond`), or at or before it.
fn clip(polygon: &[[f64; 2]], axis: usize, at: f64, beyond: bool) -> Vec<[f64; 2]> {
    let inside = |p: &[f64; 2]| if beyond { p[axis] >= at } else { p[axis] <= at };
    let mut out = Vec::with_capacity(polygon.len() + 4);
    for k in 0..polygon.len() {
        let (a, b) = (polygon[k], polygon[(k + 1) % polygon.len()]);
        if inside(&a) {
            out.push(a);
        }
        if inside(&a) != inside(&b) {
            let s = (at - a[axis]) / (b[axis] - a[axis]);
            let mut p = [a[0] + s * (b[0] - a[0]), a[1] + s * (b[1] - a[1])];
            p[axis] = at;
            out.push(p);
        }
    }
    out
}

/// Triangulates the faces of a shape into `layer`: placed by `place`, millimetres to metres.
pub(super) fn triangulate(text: &str, place: &DMat4, layer: &mut Layer) {
    let Some(shape) = Brep::parse(text) else { return };
    let mut faces = Vec::new();
    shape.faces(shape.root.0, shape.location(shape.root.2), shape.root.1, 0, &mut faces);
    for (face, at, reversed) in faces {
        shape.face(face, &(*place * at), reversed, layer);
    }
}

/// The words of the file.
struct Words<'a>(std::str::SplitAsciiWhitespace<'a>);

impl<'a> Words<'a> {
    fn word(&mut self) -> Option<&'a str> {
        self.0.next()
    }

    fn real(&mut self) -> Option<f64> {
        self.word()?.parse::<f64>().ok().filter(|v| v.is_finite())
    }

    fn int(&mut self) -> Option<i64> {
        self.word()?.parse().ok()
    }

    fn count(&mut self) -> Option<usize> {
        self.int().and_then(|n| usize::try_from(n).ok()).filter(|n| *n < 50_000_000)
    }

    fn vec(&mut self, dim: usize) -> Option<DVec3> {
        Some(DVec3::new(self.real()?, self.real()?, if dim == 3 { self.real()? } else { 0.0 }))
    }

    /// Steps over the heading of a section and gives its count.
    fn section(&mut self, name: &str) -> Option<usize> {
        (self.word()? == name).then_some(())?;
        self.count()
    }
}

/// The basis functions of a B-spline in one direction.
#[derive(Debug, Clone)]
struct Basis {
    degree: usize,
    /// The knots, each as often as its multiplicity; of a periodic one, run on at both ends.
    flat: Vec<f64>,
    /// How many poles there are: a periodic basis takes them round.
    poles: usize,
}

impl Basis {
    /// From knots with their multiplicities.
    fn new(degree: usize, knots: &[(f64, usize)], poles: usize, periodic: bool) -> Option<Basis> {
        let expand = |knots: &[(f64, usize)]| knots.iter().flat_map(|(k, m)| std::iter::repeat_n(*k, *m)).collect::<Vec<f64>>();
        if poles == 0 || knots.len() < 2 || degree == 0 || degree > 25 {
            return None;
        }
        if !periodic {
            let flat = expand(knots);
            return (flat.len() == poles + degree + 1).then_some(Basis { degree, flat, poles });
        }
        // one period of knots, repeated before and after it
        let base = expand(&knots[..knots.len() - 1]);
        let period = knots[knots.len() - 1].0 - knots[0].0;
        let first = knots[0].1.min(degree) as isize;
        if base.len() != poles || period <= 0.0 {
            return None;
        }
        let n = poles as isize;
        let from = first - degree as isize - 1;
        let flat = (from..=n + degree as isize).map(|i| base[i.rem_euclid(n) as usize] + period * i.div_euclid(n) as f64).collect();
        Some(Basis { degree, flat, poles })
    }

    /// The range it is defined on.
    fn range(&self) -> (f64, f64) {
        (self.flat[self.degree], self.flat[self.flat.len() - self.degree - 1])
    }

    /// The poles that count at `t` (the first of them; they follow one another, round a periodic
    /// basis) and their weights there.
    fn at(&self, t: f64) -> (usize, Vec<f64>) {
        let (p, (lo, hi)) = (self.degree, self.range());
        let t = t.clamp(lo, hi);
        let last = self.flat.len() - p - 2;
        // the span: the last knot at or before t, within the range
        let mut k = self.flat.partition_point(|x| *x <= t).saturating_sub(1).clamp(p, last);
        while k > p && self.flat[k] >= self.flat[k + 1] {
            k -= 1;
        }
        let mut n = vec![0.0; p + 1];
        n[0] = 1.0;
        let (mut left, mut right) = (vec![0.0; p + 1], vec![0.0; p + 1]);
        for j in 1..=p {
            left[j] = t - self.flat[k + 1 - j];
            right[j] = self.flat[k + j] - t;
            let mut saved = 0.0;
            for r in 0..j {
                let width = right[r + 1] + left[j - r];
                let part = if width.abs() > 1e-300 { n[r] / width } else { 0.0 };
                n[r] = saved + right[r + 1] * part;
                saved = left[j - r] * part;
            }
            n[j] = saved;
        }
        (k - p, n)
    }

    /// How far apart its knots lie, on average.
    fn span(&self) -> f64 {
        let (lo, hi) = self.range();
        let mut spans = 0;
        for w in self.flat[self.degree..self.flat.len() - self.degree].windows(2) {
            spans += (w[1] > w[0]) as usize;
        }
        (hi - lo) / spans.max(1) as f64
    }
}

/// A curve, in space or in a surface's (u, v) (then its z is 0).
#[derive(Debug, Clone)]
enum Curve {
    Line { p: DVec3, d: DVec3 },
    /// `c + a cos t x + b sin t y`.
    Ellipse { c: DVec3, x: DVec3, y: DVec3, a: f64, b: f64 },
    /// `c + t² / 4f x + t y`.
    Parabola { c: DVec3, x: DVec3, y: DVec3, focal: f64 },
    /// `c + a cosh t x + b sinh t y`.
    Hyperbola { c: DVec3, x: DVec3, y: DVec3, a: f64, b: f64 },
    Spline { basis: Basis, poles: Vec<DVec3>, weights: Option<Vec<f64>> },
    /// A curve this does not draw (an offset curve).
    Other,
}

impl Curve {
    fn read(w: &mut Words, dim: usize) -> Option<Curve> {
        let kind = w.int()?;
        // a conic's frame: its centre, in space its normal, its two axes
        let frame = |w: &mut Words| -> Option<(DVec3, DVec3, DVec3)> {
            let c = w.vec(dim)?;
            if dim == 3 {
                w.vec(3)?;
            }
            Some((c, w.vec(dim)?, w.vec(dim)?))
        };
        Some(match kind {
            1 => Curve::Line { p: w.vec(dim)?, d: w.vec(dim)? },
            2 => {
                let (c, x, y) = frame(w)?;
                let r = w.real()?;
                Curve::Ellipse { c, x, y, a: r, b: r }
            }
            3 => {
                let (c, x, y) = frame(w)?;
                Curve::Ellipse { c, x, y, a: w.real()?, b: w.real()? }
            }
            4 => {
                let (c, x, y) = frame(w)?;
                Curve::Parabola { c, x, y, focal: w.real()? }
            }
            5 => {
                let (c, x, y) = frame(w)?;
                Curve::Hyperbola { c, x, y, a: w.real()?, b: w.real()? }
            }
            6 | 7 => {
                let rational = w.int()? != 0;
                let periodic = kind == 7 && w.int()? != 0;
                let degree = w.count()?;
                let (poles_n, knots_n) = if kind == 7 { (w.count()?, w.count()?) } else { (degree + 1, 2) };
                let mut poles = Vec::with_capacity(poles_n);
                let mut weights = Vec::new();
                for _ in 0..poles_n {
                    poles.push(w.vec(dim)?);
                    if rational {
                        weights.push(w.real()?);
                    }
                }
                let knots: Vec<(f64, usize)> = if kind == 7 { (0..knots_n).map(|_| Some((w.real()?, w.count()?))).collect::<Option<_>>()? } else { vec![(0.0, degree + 1), (1.0, degree + 1)] };
                match Basis::new(degree, &knots, poles_n, periodic) {
                    Some(basis) => Curve::Spline { basis, poles, weights: rational.then_some(weights) },
                    None => Curve::Other,
                }
            }
            // a part of a curve: an edge has its own range
            8 => {
                w.real()?;
                w.real()?;
                Curve::read(w, dim)?
            }
            9 => {
                w.real()?;
                if dim == 3 {
                    w.vec(3)?;
                }
                Curve::read(w, dim)?;
                Curve::Other
            }
            _ => return None,
        })
    }

    fn at(&self, t: f64) -> Option<DVec3> {
        Some(match self {
            Curve::Line { p, d } => *p + *d * t,
            Curve::Ellipse { c, x, y, a, b } => *c + *x * (a * t.cos()) + *y * (b * t.sin()),
            Curve::Parabola { c, x, y, focal } => *c + *x * (t * t / (4.0 * focal)) + *y * t,
            Curve::Hyperbola { c, x, y, a, b } => *c + *x * (a * t.cosh()) + *y * (b * t.sinh()),
            Curve::Spline { basis, poles, weights } => {
                let (first, n) = basis.at(t);
                let (mut sum, mut total) = (DVec3::ZERO, 0.0);
                for (k, n) in n.iter().enumerate() {
                    let i = (first + k) % basis.poles;
                    let weight = n * weights.as_ref().map_or(1.0, |w| w[i]);
                    sum += poles[i] * weight;
                    total += weight;
                }
                (total.abs() > 1e-300).then(|| sum / total)?
            }
            Curve::Other => return None,
        })
    }

    /// The step of its parameter it is cut in; none for a straight line.
    fn step(&self) -> Option<f64> {
        match self {
            Curve::Line { .. } | Curve::Other => None,
            Curve::Ellipse { .. } => Some(STEP),
            Curve::Parabola { .. } | Curve::Hyperbola { .. } => Some(0.25),
            Curve::Spline { basis, .. } => Some(basis.span() / (basis.degree as f64 * 2.0).max(2.0)),
        }
    }
}

/// A surface: a point of it per (u, v).
#[derive(Debug, Clone)]
enum Surface {
    Plane { o: DVec3, x: DVec3, y: DVec3 },
    /// `o + r (cos u x + sin u y) + v z`.
    Cylinder { o: DVec3, z: DVec3, x: DVec3, y: DVec3, r: f64 },
    /// `o + (r + v sin a)(cos u x + sin u y) + v cos a z`.
    Cone { o: DVec3, z: DVec3, x: DVec3, y: DVec3, r: f64, angle: f64 },
    /// `o + r cos v (cos u x + sin u y) + r sin v z`.
    Sphere { o: DVec3, z: DVec3, x: DVec3, y: DVec3, r: f64 },
    /// `o + (major + minor cos v)(cos u x + sin u y) + minor sin v z`.
    Torus { o: DVec3, z: DVec3, x: DVec3, y: DVec3, major: f64, minor: f64 },
    /// `curve(u) + v d`.
    Extrusion { d: DVec3, curve: Curve },
    /// `curve(v)` turned by u about the axis through `o` along `d`.
    Revolution { o: DVec3, d: DVec3, curve: Curve },
    Spline { u: Basis, v: Basis, poles: Vec<DVec3>, weights: Option<Vec<f64>> },
    Other,
}

impl Surface {
    fn read(w: &mut Words) -> Option<Surface> {
        let kind = w.int()?;
        let frame = |w: &mut Words| Some((w.vec(3)?, w.vec(3)?, w.vec(3)?, w.vec(3)?));
        Some(match kind {
            1 => {
                let (o, _, x, y) = frame(w)?;
                Surface::Plane { o, x, y }
            }
            2 => {
                let (o, z, x, y) = frame(w)?;
                Surface::Cylinder { o, z, x, y, r: w.real()? }
            }
            3 => {
                let (o, z, x, y) = frame(w)?;
                Surface::Cone { o, z, x, y, r: w.real()?, angle: w.real()? }
            }
            4 => {
                let (o, z, x, y) = frame(w)?;
                Surface::Sphere { o, z, x, y, r: w.real()? }
            }
            5 => {
                let (o, z, x, y) = frame(w)?;
                Surface::Torus { o, z, x, y, major: w.real()?, minor: w.real()? }
            }
            6 => Surface::Extrusion { d: w.vec(3)?, curve: Curve::read(w, 3)? },
            7 => Surface::Revolution { o: w.vec(3)?, d: w.vec(3)?.try_normalize()?, curve: Curve::read(w, 3)? },
            8 | 9 => {
                let rational = (w.int()? != 0) | (w.int()? != 0);
                let periodic = if kind == 9 { (w.int()? != 0, w.int()? != 0) } else { (false, false) };
                let degree = (w.count()?, w.count()?);
                let (poles_n, knots_n) = if kind == 9 { ((w.count()?, w.count()?), (w.count()?, w.count()?)) } else { ((degree.0 + 1, degree.1 + 1), (2, 2)) };
                let mut poles = Vec::with_capacity(poles_n.0 * poles_n.1);
                let mut weights = Vec::new();
                for _ in 0..poles_n.0 * poles_n.1 {
                    poles.push(w.vec(3)?);
                    if rational {
                        weights.push(w.real()?);
                    }
                }
                let mut knots = |n: usize, degree: usize| -> Option<Vec<(f64, usize)>> {
                    if kind == 9 { (0..n).map(|_| Some((w.real()?, w.count()?))).collect() } else { Some(vec![(0.0, degree + 1), (1.0, degree + 1)]) }
                };
                let (ku, kv) = (knots(knots_n.0, degree.0)?, knots(knots_n.1, degree.1)?);
                match (Basis::new(degree.0, &ku, poles_n.0, periodic.0), Basis::new(degree.1, &kv, poles_n.1, periodic.1)) {
                    (Some(u), Some(v)) => Surface::Spline { u, v, poles, weights: rational.then_some(weights) },
                    _ => Surface::Other,
                }
            }
            // a part of a surface: a face has its own wires
            10 => {
                for _ in 0..4 {
                    w.real()?;
                }
                Surface::read(w)?
            }
            11 => {
                w.real()?;
                Surface::read(w)?;
                Surface::Other
            }
            _ => return None,
        })
    }

    fn at(&self, u: f64, v: f64) -> Option<DVec3> {
        Some(match self {
            Surface::Plane { o, x, y } => *o + *x * u + *y * v,
            Surface::Cylinder { o, z, x, y, r } => *o + (*x * u.cos() + *y * u.sin()) * *r + *z * v,
            Surface::Cone { o, z, x, y, r, angle } => *o + (*x * u.cos() + *y * u.sin()) * (r + v * angle.sin()) + *z * (v * angle.cos()),
            Surface::Sphere { o, z, x, y, r } => *o + (*x * u.cos() + *y * u.sin()) * (r * v.cos()) + *z * (r * v.sin()),
            Surface::Torus { o, z, x, y, major, minor } => *o + (*x * u.cos() + *y * u.sin()) * (major + minor * v.cos()) + *z * (minor * v.sin()),
            Surface::Extrusion { d, curve } => curve.at(u)? + *d * v,
            Surface::Revolution { o, d, curve } => {
                // Rodrigues: the point turned by u about the axis
                let p = curve.at(v)? - *o;
                *o + p * u.cos() + d.cross(p) * u.sin() + *d * (d.dot(p) * (1.0 - u.cos()))
            }
            Surface::Spline { u: bu, v: bv, poles, weights } => {
                let ((iu, nu), (iv, nv)) = (bu.at(u), bv.at(v));
                let (mut sum, mut total) = (DVec3::ZERO, 0.0);
                for (a, nu) in nu.iter().enumerate() {
                    for (b, nv) in nv.iter().enumerate() {
                        let i = ((iu + a) % bu.poles) * bv.poles + (iv + b) % bv.poles;
                        let weight = nu * nv * weights.as_ref().map_or(1.0, |w| w[i]);
                        sum += *poles.get(i)? * weight;
                        total += weight;
                    }
                }
                (total.abs() > 1e-300).then(|| sum / total)?
            }
            Surface::Other => return None,
        })
    }

    /// The steps of u and v a triangle may span; none along a straight direction.
    fn steps(&self) -> (Option<f64>, Option<f64>) {
        match self {
            Surface::Plane { .. } | Surface::Other => (None, None),
            Surface::Cylinder { .. } | Surface::Cone { .. } => (Some(STEP), None),
            Surface::Sphere { .. } | Surface::Torus { .. } => (Some(STEP), Some(STEP)),
            Surface::Extrusion { curve, .. } => (curve.step(), None),
            Surface::Revolution { curve, .. } => (Some(STEP), curve.step()),
            Surface::Spline { u, v, .. } => (Some(u.span() / (u.degree as f64 * 2.0).max(2.0)), Some(v.span() / (v.degree as f64 * 2.0).max(2.0))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Vertex,
    Edge,
    Wire,
    Face,
    Other,
}

/// An edge's curve in a surface: on a surface that closes on itself, the seam has two.
#[derive(Debug, Clone, Copy)]
struct OnSurface {
    curve: usize,
    other: Option<usize>,
    surface: usize,
    location: usize,
    range: (f64, f64),
}

#[derive(Debug, Clone)]
struct Shape {
    kind: Kind,
    /// Its parts: the shape, whether it is turned round, and its location.
    parts: Vec<(usize, bool, usize)>,
    /// An edge's 3D curve, with its location and range.
    curve: Option<(usize, usize, (f64, f64))>,
    on: Vec<OnSurface>,
    /// A face's surface and its location.
    surface: Option<(usize, usize)>,
}

struct Brep {
    locations: Vec<DMat4>,
    curves2d: Vec<Curve>,
    curves: Vec<Curve>,
    surfaces: Vec<Surface>,
    shapes: Vec<Shape>,
    /// The root: the shape, whether it is turned round, and its location.
    root: (usize, bool, usize),
}

impl Brep {
    fn parse(text: &str) -> Option<Brep> {
        let version: usize = text.split_once("Topology V")?.1.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()?;
        let mut w = Words(text.split_once("Locations")?.1.split_ascii_whitespace());
        let count = w.count()?;
        let mut locations: Vec<DMat4> = Vec::with_capacity(count);
        for _ in 0..count {
            let m = match w.int()? {
                1 => {
                    let r: Vec<f64> = (0..12).map(|_| w.real()).collect::<Option<_>>()?;
                    DMat4::from_cols_array(&[r[0], r[4], r[8], 0.0, r[1], r[5], r[9], 0.0, r[2], r[6], r[10], 0.0, r[3], r[7], r[11], 1.0])
                }
                2 => {
                    // a product of powers of earlier ones, the first applied first
                    let mut m = DMat4::IDENTITY;
                    loop {
                        let i = w.count()?;
                        if i == 0 {
                            break;
                        }
                        let (base, power) = (*locations.get(i - 1)?, w.int()?);
                        let base = if power < 0 { base.inverse() } else { base };
                        for _ in 0..power.unsigned_abs().min(64) {
                            m = base * m;
                        }
                    }
                    m
                }
                _ => return None,
            };
            locations.push(m);
        }
        let count = w.section("Curve2ds")?;
        let curves2d = (0..count).map(|_| Curve::read(&mut w, 2)).collect::<Option<Vec<_>>>()?;
        let count = w.section("Curves")?;
        let curves = (0..count).map(|_| Curve::read(&mut w, 3)).collect::<Option<Vec<_>>>()?;
        // polygons are only kept with a triangulation, which FreeCAD leaves out
        if w.section("Polygon3D")? != 0 || w.section("PolygonOnTriangulations")? != 0 {
            return None;
        }
        let count = w.section("Surfaces")?;
        let surfaces = (0..count).map(|_| Surface::read(&mut w)).collect::<Option<Vec<_>>>()?;
        if w.section("Triangulations")? != 0 {
            return None;
        }
        let count = w.section("TShapes")?;
        let mut shapes = Vec::with_capacity(count);
        // a part as `+12` or `-3`: counted back from the last shape; `i` and `e` are inside or outside
        let part = |word: &str, location: Option<i64>, count: usize| -> Option<(usize, bool, usize, bool)> {
            let number: usize = word.get(1..)?.parse().ok()?;
            Some((count.checked_sub(number)?, word.starts_with('-'), usize::try_from(location?).ok()?, word.starts_with(['+', '-'])))
        };
        for _ in 0..count {
            let mut shape = Shape { kind: Kind::Other, parts: Vec::new(), curve: None, on: Vec::new(), surface: None };
            match w.word()? {
                "Ve" => {
                    shape.kind = Kind::Vertex;
                    w.real()?;
                    w.vec(3)?;
                    // what it lies on: a parameter and a kind, to a kind of 0
                    loop {
                        w.real()?;
                        match w.int()? {
                            0 => break,
                            1 => drop((w.int()?, w.int()?)),
                            2 => drop((w.int()?, w.int()?, w.int()?)),
                            3 => drop((w.real()?, w.int()?, w.int()?)),
                            _ => return None,
                        }
                    }
                }
                "Ed" => {
                    shape.kind = Kind::Edge;
                    w.real()?;
                    let _ = (w.int()?, w.int()?, w.int()?);
                    loop {
                        match w.int()? {
                            0 => break,
                            1 => shape.curve = Some((w.count()?, w.count()?, (w.real()?, w.real()?))),
                            2 => {
                                shape.on.push(OnSurface { curve: w.count()?, other: None, surface: w.count()?, location: w.count()?, range: (w.real()?, w.real()?) });
                                if version >= 2 {
                                    for _ in 0..4 {
                                        w.real()?;
                                    }
                                }
                            }
                            3 => {
                                let curve = w.count()?;
                                // the second curve and the continuity come as one word: `2CN`
                                let word = w.word()?;
                                let digits = word.chars().take_while(char::is_ascii_digit).count();
                                let other: usize = word[..digits].parse().ok()?;
                                if digits == word.len() {
                                    w.word()?;
                                }
                                shape.on.push(OnSurface { curve, other: Some(other), surface: w.count()?, location: w.count()?, range: (w.real()?, w.real()?) });
                                if version >= 2 {
                                    for _ in 0..8 {
                                        w.real()?;
                                    }
                                }
                            }
                            4 => {
                                w.word()?;
                                let _ = (w.int()?, w.int()?, w.int()?, w.int()?);
                            }
                            _ => return None,
                        }
                    }
                }
                "Wi" => shape.kind = Kind::Wire,
                "Fa" => {
                    shape.kind = Kind::Face;
                    w.int()?;
                    w.real()?;
                    shape.surface = Some((w.count()?, w.count()?));
                }
                "Sh" | "So" | "CS" | "Co" => {}
                _ => return None,
            }
            // its flags (seven digits; a face may name a triangulation first), then its parts to `*`
            let mut word = w.word()?;
            if word == "2" {
                w.int()?;
                word = w.word()?;
            }
            (word.len() == 7).then_some(())?;
            loop {
                let word = w.word()?;
                if word == "*" {
                    break;
                }
                let (shape_at, reversed, location, bounds) = part(word, w.int(), count)?;
                if bounds {
                    shape.parts.push((shape_at, reversed, location));
                }
            }
            shapes.push(shape);
        }
        let (root, reversed, location, _) = part(w.word()?, w.int(), count)?;
        Some(Brep { locations, curves2d, curves, surfaces, shapes, root: (root, reversed, location) })
    }

    fn location(&self, i: usize) -> DMat4 {
        i.checked_sub(1).and_then(|i| self.locations.get(i)).copied().unwrap_or(DMat4::IDENTITY)
    }

    /// The faces under a shape: each with where it stands and whether it is turned round.
    fn faces(&self, at: usize, place: DMat4, reversed: bool, depth: usize, out: &mut Vec<(usize, DMat4, bool)>) {
        let Some(shape) = self.shapes.get(at).filter(|_| depth < 64) else { return };
        if shape.kind == Kind::Face {
            out.push((at, place, reversed));
            return;
        }
        for &(part, turned, location) in &shape.parts {
            self.faces(part, place * self.location(location), reversed ^ turned, depth + 1, out);
        }
    }

    /// The wires of a face as polygons in its surface's (u, v).
    fn loops(&self, face: &Shape, surface: &Surface, key: (usize, usize)) -> Option<Vec<Vec<[f64; 2]>>> {
        let own = self.location(key.1);
        let mut loops = Vec::new();
        for &(wire_at, wire_turned, wire_place) in &face.parts {
            let wire = self.shapes.get(wire_at).filter(|s| s.kind == Kind::Wire)?;
            let mut runs: Vec<Vec<[f64; 2]>> = Vec::new();
            let mut edges: Vec<&(usize, bool, usize)> = wire.parts.iter().collect();
            if wire_turned {
                edges.reverse();
            }
            for &&(edge_at, edge_turned, edge_place) in &edges {
                let edge = self.shapes.get(edge_at).filter(|s| s.kind == Kind::Edge)?;
                let turned = wire_turned ^ edge_turned;
                // its curve in this surface: the one whose location, with the edge's own, is the face's
                let wanted = (self.location(wire_place) * self.location(edge_place)).inverse() * own;
                let here: Vec<&OnSurface> = edge.on.iter().filter(|o| o.surface == key.0).collect();
                let on = here.iter().find(|o| self.location(o.location).abs_diff_eq(wanted, 1e-6)).or_else(|| here.first().filter(|_| here.len() == 1 && !matches!(surface, Surface::Plane { .. }))).copied();
                let space = edge.curve.and_then(|(c, l, range)| Some((self.curves.get(c.checked_sub(1)?)?, l, range)));
                // as many pieces as its curve in space bends, so both faces at an edge cut it alike
                let pieces = |step: Option<f64>, (from, to): (f64, f64)| step.map_or(1, |s| ((to - from).abs() / s - 1e-9).ceil().clamp(1.0, 400.0) as usize);
                let mut points: Vec<[f64; 2]> = match (on, space, surface) {
                    (Some(on), ..) => {
                        // the seam of a closed surface: its first curve one way, its second the other
                        let curve = self.curves2d.get(if turned { on.other.unwrap_or(on.curve) } else { on.curve }.checked_sub(1)?)?;
                        let n = pieces(space.and_then(|(c, ..)| c.step()).or_else(|| curve.step()), on.range);
                        (0..=n).map(|k| curve.at(on.range.0 + (on.range.1 - on.range.0) * k as f64 / n as f64).map(|p| [p.x, p.y])).collect::<Option<_>>()?
                    }
                    // a plane needs no curves of its own: its edges' curves in space, laid on it
                    (None, Some((curve, location, range)), Surface::Plane { o, x, y }) => {
                        let to_plane = own.inverse() * self.location(wire_place) * self.location(edge_place) * self.location(location);
                        let n = pieces(curve.step(), range);
                        (0..=n)
                            .map(|k| {
                                let p = to_plane.transform_point3(curve.at(range.0 + (range.1 - range.0) * k as f64 / n as f64)?) - *o;
                                Some([p.dot(*x) / x.length_squared(), p.dot(*y) / y.length_squared()])
                            })
                            .collect::<Option<_>>()?
                    }
                    _ => return None,
                };
                if turned {
                    points.reverse();
                }
                runs.push(points);
            }
            // a wire lists its edges in no order: each follows the one that ends where it starts
            let extent = |k: usize| runs.iter().flatten().map(|p| p[k]).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)));
            let scale = [0, 1].map(|k| (extent(k).1 - extent(k).0).max(1e-12));
            let apart = |a: [f64; 2], b: [f64; 2]| ((a[0] - b[0]) / scale[0]).hypot((a[1] - b[1]) / scale[1]);
            let mut polygon: Vec<[f64; 2]> = Vec::new();
            let mut end: Option<[f64; 2]> = None;
            while !runs.is_empty() {
                let next = end.map_or(0, |end| (0..runs.len()).min_by(|a, b| apart(runs[*a][0], end).total_cmp(&apart(runs[*b][0], end))).unwrap_or(0));
                let mut points = runs.remove(next);
                // its last point is the next edge's first
                end = points.pop();
                polygon.extend(points);
            }
            if polygon.len() >= 3 {
                loops.push(polygon);
            }
        }
        Some(loops)
    }

    /// Triangulates a face into `layer`.
    fn face(&self, at: usize, place: &DMat4, reversed: bool, layer: &mut Layer) -> Option<()> {
        let face = self.shapes.get(at)?;
        let key = face.surface?;
        let surface = self.surfaces.get(key.0.checked_sub(1)?)?;
        let mut loops = self.loops(face, surface, key)?;
        if loops.is_empty() {
            return None;
        }
        // the largest is the outside, the others are holes
        let area = |l: &[[f64; 2]]| (0..l.len()).map(|k| l[k][0] * l[(k + 1) % l.len()][1] - l[(k + 1) % l.len()][0] * l[k][1]).sum::<f64>() / 2.0;
        loops.sort_by(|a, b| area(b).abs().total_cmp(&area(a).abs()));
        let (du, dv) = surface.steps();
        // (u, v) in steps, so a triangle is as wide as it is high where the surface bends alike
        let extent = |k: usize| loops.iter().flatten().map(|p| p[k]).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)));
        let unit = |step: Option<f64>, k: usize| step.unwrap_or_else(|| (extent(k).1 - extent(k).0).max(1e-9));
        let (su, sv) = (unit(du, 0), unit(dv, 1));
        // cut into the cells of a grid of steps and triangulate each, so no triangle spans more
        // than a step; points the cells share are one point
        let strips = |step: Option<f64>, k: usize| -> Vec<Option<(f64, f64)>> {
            let Some(step) = step else { return vec![None] };
            let (lo, hi) = extent(k);
            // no more than 512 strips, however large the face
            let step = step.max((hi - lo) / 512.0);
            let (first, last) = ((lo / step + 1e-9).floor() as i64, (hi / step - 1e-9).ceil() as i64);
            (first..last.max(first + 1)).map(|i| Some((i as f64 * step, (i + 1) as f64 * step))).collect()
        };
        let between = |loops: &[Vec<[f64; 2]>], axis: usize, range: Option<(f64, f64)>| -> Vec<Vec<[f64; 2]>> {
            match range {
                None => loops.to_vec(),
                Some((lo, hi)) => loops.iter().map(|l| clip(&clip(l, axis, lo, true), axis, hi, false)).collect(),
            }
        };
        let mut uv: Vec<[f64; 2]> = Vec::new();
        let mut welded: HashMap<(i64, i64), usize> = HashMap::new();
        let mut triangles: Vec<[usize; 3]> = Vec::new();
        for u_range in strips(du, 0) {
            let strip = between(&loops, 0, u_range);
            if strip[0].len() < 3 {
                continue;
            }
            for v_range in strips(dv, 1) {
                let cell = between(&strip, 1, v_range);
                if cell[0].len() < 3 {
                    continue;
                }
                let mut flat = Vec::new();
                let mut holes = Vec::new();
                let mut points = Vec::new();
                for (k, l) in cell.iter().enumerate().filter(|(k, l)| *k == 0 || (l.len() >= 3 && area(l).abs() > 1e-12 * su * sv)) {
                    if k > 0 {
                        holes.push(flat.len() / 2);
                    }
                    flat.extend(l.iter().flat_map(|p| [p[0] / su, p[1] / sv]));
                    points.extend(l.iter().copied());
                }
                let Ok(cut) = earcutr::earcut(&flat, &holes, 2) else { continue };
                let mut point = |p: [f64; 2]| {
                    *welded.entry(((p[0] / su * 1e7).round() as i64, (p[1] / sv * 1e7).round() as i64)).or_insert_with(|| {
                        uv.push(p);
                        uv.len() - 1
                    })
                };
                triangles.extend(cut.chunks_exact(3).map(|t| [point(points[t[0]]), point(points[t[1]]), point(points[t[2]])]).filter(|t| t[0] != t[1] && t[1] != t[2] && t[2] != t[0]));
            }
        }
        // the points and normals of the surface, where the face stands
        let to_world = *place * self.location(key.1);
        let mirrored = to_world.determinant() < 0.0;
        let (hu, hv) = (du.unwrap_or(su) * 1e-3, dv.unwrap_or(sv) * 1e-3);
        let base = (layer.positions.len() / 3) as u32;
        let mut points = Vec::with_capacity(uv.len());
        for p in &uv {
            let at = surface.at(p[0], p[1])?;
            let normal = (surface.at(p[0] + hu, p[1])? - surface.at(p[0] - hu, p[1])?).cross(surface.at(p[0], p[1] + hv)? - surface.at(p[0], p[1] - hv)?);
            let normal = to_world.transform_vector3(normal).try_normalize().map(|n| if reversed ^ mirrored { -n } else { n });
            points.push((to_world.transform_point3(at) * 0.001, normal));
        }
        let first = layer.indices.len();
        let mut flat_normal: Vec<Option<DVec3>> = vec![None; points.len()];
        for t in &triangles {
            // anticlockwise in (u, v) looks along the surface's normal
            let turn = (uv[t[1]][0] - uv[t[0]][0]) * (uv[t[2]][1] - uv[t[0]][1]) - (uv[t[2]][0] - uv[t[0]][0]) * (uv[t[1]][1] - uv[t[0]][1]);
            let t = if (turn < 0.0) ^ reversed ^ mirrored { [t[0], t[2], t[1]] } else { *t };
            let [a, b, c] = t.map(|i| points[i].0);
            let Some(normal) = (b - a).cross(c - a).try_normalize() else { continue };
            for i in t {
                flat_normal[i].get_or_insert(normal);
            }
            layer.indices.extend(t.map(|i| base + i as u32));
        }
        if layer.indices.len() == first {
            return None;
        }
        for (k, (p, normal)) in points.iter().enumerate() {
            // at a pole the surface has no normal of its own: its triangle's
            let normal = normal.or(flat_normal[k]).unwrap_or(DVec3::Z);
            layer.positions.extend(p.to_array().map(|x| x as f32));
            layer.normals.extend(normal.to_array().map(|x| x as f32));
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clamped_basis_is_a_bezier_and_a_periodic_one_closes() {
        // a quadratic Bézier: weights (1 - t)², 2t(1 - t), t²
        let b = Basis::new(2, &[(0.0, 3), (1.0, 3)], 3, false).unwrap();
        let (first, n) = b.at(0.25);
        assert_eq!(first, 0);
        assert!((n[0] - 0.5625).abs() < 1e-12 && (n[1] - 0.375).abs() < 1e-12 && (n[2] - 0.0625).abs() < 1e-12, "{n:?}");
        // a periodic cubic through four poles: where it ends is where it starts
        let knots: Vec<(f64, usize)> = (0..=4).map(|k| (k as f64, 1)).collect();
        let basis = Basis::new(3, &knots, 4, true).unwrap();
        assert_eq!(basis.range(), (0.0, 4.0));
        let poles = vec![DVec3::new(1.0, 0.0, 0.0), DVec3::new(0.0, 1.0, 0.0), DVec3::new(-1.0, 0.0, 0.0), DVec3::new(0.0, -1.0, 0.0)];
        let curve = Curve::Spline { basis, poles, weights: None };
        let (a, b) = (curve.at(0.0).unwrap(), curve.at(4.0).unwrap());
        assert!((a - b).length() < 1e-12, "{a} {b}");
        assert!((curve.at(1.0).unwrap() - curve.at(3.0).unwrap()).length() > 0.5);
    }

    #[test]
    fn what_is_no_shape_gives_nothing() {
        let mut layer = Layer::default();
        triangulate("", &DMat4::IDENTITY, &mut layer);
        triangulate("CASCADE Topology V1\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n", &DMat4::IDENTITY, &mut layer);
        assert!(layer.indices.is_empty());
    }
}



