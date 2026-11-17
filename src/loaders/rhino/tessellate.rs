//! Meshes for Rhino polysurfaces and extrusions saved without their render meshes, as a script,
//! another program or "save small" leaves them: the viewer meshes the exact geometry itself, as
//! Rhino does when it opens such a file. Read as openNURBS writes it (`ON_Brep`, `ON_Extrusion`,
//! the curve and surface classes): trim curves, surfaces, trims, loops and faces.
//!
//! A face is meshed in the parameter plane of its surface. An untrimmed face is a grid whose
//! lines follow the bending and twist of the surface and lie on its kinks. A trimmed face's
//! loops are sampled until the points on the surface lie within the tolerance of their chords
//! (and at least once per grid cell on a curved surface), and a constrained Delaunay
//! triangulation of the loops and the grid points inside them is kept within the loops. Planes,
//! NURBS surfaces (rational too), surfaces of revolution and sum surfaces are read, also as
//! objects of their own; curves are lines, arcs, polylines, NURBS curves and polycurves.
//! Extrusions are walls along their profiles and, when capped, flat caps, mitered ends
//! included. Other surfaces (offset surfaces, proxies) are left out.

use super::{err, Cur, Mesh, Uuid, R};
use glam::{DVec2, DVec3};
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};

const ON_LINE_CURVE: Uuid = Uuid::parse("4ED7D4DB-E947-11d3-BFE5-0010830122F0");
const ON_ARC_CURVE: Uuid = Uuid::parse("CF33BE2A-09B4-11d4-BFFB-0010830122F0");
const ON_POLYLINE_CURVE: Uuid = Uuid::parse("4ED7D4E6-E947-11d3-BFE5-0010830122F0");
/// `ON_NurbsCurve` and the ids older files write it with (`TL_NurbsCurve`, ..).
const ON_NURBS_CURVE: [Uuid; 3] = [
    Uuid::parse("4ED7D4DD-E947-11d3-BFE5-0010830122F0"),
    Uuid::parse("5EAF1119-0B51-11d4-BFFE-0010830122F0"),
    Uuid::parse("76A709D5-1550-11d4-8000-0010830122F0"),
];
const ON_POLY_CURVE: [Uuid; 2] = [Uuid::parse("4ED7D4E0-E947-11d3-BFE5-0010830122F0"), Uuid::parse("EF638317-154B-11d4-8000-0010830122F0")];
const ON_PLANE_SURFACE: Uuid = Uuid::parse("4ED7D4DF-E947-11d3-BFE5-0010830122F0");
const ON_NURBS_SURFACE: [Uuid; 3] = [
    Uuid::parse("4ED7D4DE-E947-11d3-BFE5-0010830122F0"),
    Uuid::parse("4760C817-0BE3-11d4-BFFE-0010830122F0"),
    Uuid::parse("FA4FD4B5-1613-11d4-8000-0010830122F0"),
];
const ON_REV_SURFACE: [Uuid; 2] = [Uuid::parse("A16220D3-163B-11d4-8000-0010830122F0"), Uuid::parse("0A8401B6-4D34-4b99-8615-1B4E723DC4E5")];
const ON_SUM_SURFACE: [Uuid; 2] = [Uuid::parse("C4CD5359-446D-4690-9FF5-29059732472B"), Uuid::parse("665F6331-2A66-4cce-81D0-B5EEBD9B5417")];

/// The highest NURBS order read (degree 31): Rhino writes up to degree 11.
const MAX_ORDER: usize = 32;
/// Polycurves nested deeper than this are not read.
const MAX_CURVE_DEPTH: usize = 8;
/// Largest angle (radians) a mesh edge may turn over on a curved surface or along a curve.
const MAX_ANGLE: f64 = 0.16;
/// Largest distance between a curve and its chord, relative to the chord.
const CHORD_RATIO: f64 = 0.02;
/// Most grid lines in one direction of a face, and most samples of one trim or profile segment.
const MAX_GRID: usize = 128;
const MAX_DEPTH: u32 = 10;

// ---------------------------------------------------------------------------------------------
// Curves

#[derive(Debug, Clone)]
pub(super) struct Nurbs {
    order: usize,
    /// The full knot vector (order + count knots): openNURBS leaves out the first and last.
    knots: Vec<f64>,
    /// Homogeneous control points: x·w, y·w, z·w, w.
    cvs: Vec<[f64; 4]>,
}

impl Nurbs {
    fn new(order: usize, knots: &[f64], cvs: Vec<[f64; 4]>) -> R<Nurbs> {
        if !(2..=MAX_ORDER).contains(&order) || cvs.len() < order || knots.len() != order + cvs.len() - 2 {
            return err("a NURBS of an unknown form");
        }
        let mut full = Vec::with_capacity(knots.len() + 2);
        full.push(knots[0]);
        full.extend_from_slice(knots);
        full.push(knots[knots.len() - 1]);
        if full.iter().any(|k| !k.is_finite()) || full.windows(2).any(|w| w[1] < w[0]) || cvs.iter().flatten().any(|v| !v.is_finite()) {
            return err("a NURBS with invalid knots or control points");
        }
        let n = Nurbs { order, knots: full, cvs };
        let [a, b] = n.domain();
        if b <= a {
            return err("a NURBS with an empty domain");
        }
        Ok(n)
    }

    fn degree(&self) -> usize {
        self.order - 1
    }

    fn domain(&self) -> [f64; 2] {
        let p = self.degree();
        [self.knots[p], self.knots[self.cvs.len()]]
    }

    /// The span `i` (p ≤ i < count) with `knots[i] ≤ t < knots[i + 1]`, clamped to the domain.
    fn span(&self, t: f64) -> usize {
        span(&self.knots, self.degree(), self.cvs.len(), t)
    }

    /// The knots inside the domain where the curve is only continuous (multiplicity of at least
    /// the degree): its kinks.
    fn kinks(&self) -> Vec<f64> {
        kinks(&self.knots, self.degree(), self.domain())
    }

    /// The distinct knots inside the domain, each span split in `degree` parts.
    fn breaks(&self) -> Vec<f64> {
        let [a, b] = self.domain();
        let mut knots: Vec<f64> = self.knots.iter().copied().filter(|&k| k >= a && k <= b).collect();
        knots.dedup();
        let parts = self.degree().max(1);
        let mut out = Vec::new();
        for w in knots.windows(2) {
            for k in 0..parts {
                out.push(w[0] + (w[1] - w[0]) * k as f64 / parts as f64);
            }
        }
        out.push(b);
        out
    }

    fn point(&self, t: f64) -> DVec3 {
        let i = self.span(t);
        let p = self.degree();
        let mut d = [[0.0; 4]; MAX_ORDER];
        d[..=p].copy_from_slice(&self.cvs[i - p..=i]);
        homogeneous(de_boor(p, &self.knots, i, &mut d[..=p], t))
    }
}

/// The knots strictly inside `domain` repeated at least `degree` times.
fn kinks(knots: &[f64], degree: usize, domain: [f64; 2]) -> Vec<f64> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < knots.len() {
        let mut j = i;
        while j < knots.len() && knots[j] == knots[i] {
            j += 1;
        }
        if j - i >= degree.max(1) && knots[i] > domain[0] && knots[i] < domain[1] {
            out.push(knots[i]);
        }
        i = j;
    }
    out
}

fn span(knots: &[f64], p: usize, count: usize, t: f64) -> usize {
    let n = count - 1;
    if t >= knots[n + 1] {
        let mut i = n;
        while i > p && knots[i] >= knots[n + 1] {
            i -= 1;
        }
        return i;
    }
    if t <= knots[p] {
        let mut i = p;
        while i < n && knots[i + 1] <= knots[p] {
            i += 1;
        }
        return i;
    }
    let (mut lo, mut hi) = (p, n + 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if t < knots[mid] {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    lo
}

/// de Boor's algorithm on the `p + 1` control points of span `i`, in place.
fn de_boor(p: usize, knots: &[f64], i: usize, d: &mut [[f64; 4]], t: f64) -> [f64; 4] {
    for r in 1..=p {
        for j in (r..=p).rev() {
            let lo = knots[j + i - p];
            let den = knots[j + 1 + i - r] - lo;
            let a = if den == 0.0 { 0.0 } else { (t - lo) / den };
            for k in 0..4 {
                d[j][k] = (1.0 - a) * d[j - 1][k] + a * d[j][k];
            }
        }
    }
    d[p]
}

fn homogeneous(h: [f64; 4]) -> DVec3 {
    let w = if h[3] == 0.0 { 1.0 } else { h[3] };
    DVec3::new(h[0] / w, h[1] / w, h[2] / w)
}

#[derive(Debug, Clone)]
pub(super) enum Curve {
    Line { from: DVec3, to: DVec3, t: [f64; 2] },
    Arc { origin: DVec3, x: DVec3, y: DVec3, radius: f64, angle: [f64; 2], t: [f64; 2] },
    Polyline { points: Vec<DVec3>, t: Vec<f64> },
    Nurbs(Nurbs),
    Poly { segments: Vec<Curve>, t: Vec<f64> },
}

fn lerp_param(t: f64, from: [f64; 2], to: [f64; 2]) -> f64 {
    let len = from[1] - from[0];
    if len == 0.0 {
        to[0]
    } else {
        to[0] + (t - from[0]) / len * (to[1] - to[0])
    }
}

impl Curve {
    fn domain(&self) -> [f64; 2] {
        match self {
            Curve::Line { t, .. } | Curve::Arc { t, .. } => *t,
            Curve::Polyline { t, .. } | Curve::Poly { t, .. } => [t[0], t[t.len() - 1]],
            Curve::Nurbs(n) => n.domain(),
        }
    }

    pub(super) fn point(&self, s: f64) -> DVec3 {
        match self {
            Curve::Line { from, to, t } => from.lerp(*to, lerp_param(s, *t, [0.0, 1.0])),
            Curve::Arc { origin, x, y, radius, angle, t } => {
                let a = lerp_param(s, *t, *angle);
                *origin + *radius * (a.cos() * *x + a.sin() * *y)
            }
            Curve::Polyline { points, t } => {
                let k = segment(t, s);
                points[k].lerp(points[k + 1], lerp_param(s, [t[k], t[k + 1]], [0.0, 1.0]))
            }
            Curve::Nurbs(n) => n.point(s),
            Curve::Poly { segments, t } => {
                let k = segment(t, s);
                let c = &segments[k];
                c.point(lerp_param(s, [t[k], t[k + 1]], c.domain()))
            }
        }
    }

    /// Parameters inside the domain where the curve may have a kink: polyline corners, the joins
    /// of polycurve segments and knots of full multiplicity.
    fn kinks(&self) -> Vec<f64> {
        let [a, b] = self.domain();
        let inner = |t: &[f64]| t.iter().copied().filter(|&s| s > a && s < b).collect::<Vec<_>>();
        match self {
            Curve::Polyline { t, .. } => inner(t),
            Curve::Poly { segments, t } => {
                let mut out = inner(t);
                for (k, c) in segments.iter().enumerate() {
                    let d = c.domain();
                    out.extend(c.kinks().into_iter().map(|s| lerp_param(s, d, [t[k], t[k + 1]])));
                }
                out
            }
            Curve::Nurbs(n) => n.kinks(),
            _ => Vec::new(),
        }
    }

    /// Parameters to start sampling from, over the whole domain: the ends, kinks, knots and
    /// the parts of arcs and spans.
    fn breaks(&self) -> Vec<f64> {
        match self {
            Curve::Line { t, .. } => t.to_vec(),
            Curve::Arc { angle, t, .. } => {
                let n = ((angle[1] - angle[0]).abs() / (std::f64::consts::PI / 4.0)).ceil().clamp(1.0, 16.0) as usize;
                (0..=n).map(|k| t[0] + (t[1] - t[0]) * k as f64 / n as f64).collect()
            }
            Curve::Polyline { t, .. } => t.clone(),
            Curve::Nurbs(n) => n.breaks(),
            Curve::Poly { segments, t } => {
                let mut out = Vec::new();
                for (k, c) in segments.iter().enumerate() {
                    let d = c.domain();
                    out.extend(c.breaks().into_iter().map(|s| lerp_param(s, d, [t[k], t[k + 1]])));
                }
                out
            }
        }
    }

    /// Parameters on `[a, b]` (decreasing when `b < a`), where the image of the curve under
    /// `map` lies within the tolerance of its chords. Every piece between breaks is split at
    /// least `min_depth` times (on a curved surface even a straight trim bends), and, for a trim
    /// on a curved surface, until no piece spans more than one `cell` of the face's grid in
    /// the parameter plane: a boundary coarser than the grid makes long triangles that zigzag
    /// over the bending (a Schwarz lantern) and add area.
    fn sample(&self, a: f64, b: f64, tol: f64, map: &dyn Fn(DVec3) -> DVec3, min_depth: u32, cell: Option<DVec2>) -> Vec<f64> {
        let (lo, hi) = (a.min(b), a.max(b));
        let mut params: Vec<f64> = self.breaks().into_iter().filter(|&s| s > lo && s < hi).collect();
        params.push(lo);
        params.push(hi);
        params.sort_by(f64::total_cmp);
        params.dedup();
        let eval = |s: f64| {
            let c = self.point(s);
            (c, map(c))
        };
        let step = Step { tol, min_depth, cell };
        let mut out = vec![params[0]];
        for w in params.windows(2) {
            refine(w[0], eval(w[0]), w[1], eval(w[1]), &step, &eval, &mut out, 0);
        }
        if b < a {
            out.reverse();
        }
        out
    }
}

/// When [`refine`] splits a piece of curve.
struct Step {
    tol: f64,
    min_depth: u32,
    cell: Option<DVec2>,
}

/// A point of a curve and its image.
type Sample = (DVec3, DVec3);

/// Adds the parameters after `a` up to `b`, bisecting while the image is off the chord at a
/// quarter, half or three quarters of the way (the middle alone misses an S bend), while
/// shallower than the minimum depth, or while the piece spans more than a grid cell.
fn refine(a: f64, (ca, pa): Sample, b: f64, (cb, pb): Sample, step: &Step, eval: &dyn Fn(f64) -> Sample, out: &mut Vec<f64>, depth: u32) {
    let m = 0.5 * (a + b);
    let sm = eval(m);
    let chord = pb - pa;
    let len = chord.length();
    let off = |p: DVec3| if len > 0.0 { (p - pa).cross(chord).length() / len } else { (p - pa).length() };
    let worst = off(sm.1).max(off(eval(0.75 * a + 0.25 * b).1)).max(off(eval(0.25 * a + 0.75 * b).1));
    let wide = step.cell.is_some_and(|c| (cb.x - ca.x).abs() > c.x || (cb.y - ca.y).abs() > c.y);
    if depth < MAX_DEPTH && (depth < step.min_depth || wide || (worst.is_finite() && worst > step.tol.max(CHORD_RATIO * len))) {
        refine(a, (ca, pa), m, sm, step, eval, out, depth + 1);
        refine(m, sm, b, (cb, pb), step, eval, out, depth + 1);
    } else {
        out.push(b);
    }
}

/// The segment `k` with `t[k] ≤ s ≤ t[k + 1]`.
fn segment(t: &[f64], s: f64) -> usize {
    let n = t.len() - 1;
    t[1..n].partition_point(|&v| v <= s).min(n - 1)
}

fn read_point(c: &mut Cur<'_>) -> R<DVec3> {
    Ok(DVec3::new(c.f64()?, c.f64()?, c.f64()?))
}

fn read_interval(c: &mut Cur<'_>) -> R<[f64; 2]> {
    Ok([c.f64()?, c.f64()?])
}

fn finite(v: &[f64]) -> R<()> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        err("a curve or surface with an invalid number")
    }
}

/// A curve written with `WriteObject`; `None` for a null object or one of another class.
pub(super) fn read_curve_object(c: &mut Cur<'_>, depth: usize) -> R<Option<Curve>> {
    match c.object()? {
        Some(o) => read_curve(o.class, o.data, depth),
        None => Ok(None),
    }
}

fn read_curve(class: Uuid, mut c: Cur<'_>, depth: usize) -> R<Option<Curve>> {
    if depth > MAX_CURVE_DEPTH {
        return err("polycurves nested too deep");
    }
    if class == ON_LINE_CURVE {
        c.version()?;
        let (from, to, t) = (read_point(&mut c)?, read_point(&mut c)?, read_interval(&mut c)?);
        finite(&[from.x, from.y, from.z, to.x, to.y, to.z, t[0], t[1]])?;
        return Ok(Some(Curve::Line { from, to, t }));
    }
    if class == ON_ARC_CURVE {
        let (major, _) = c.version()?;
        if major != 1 {
            return Ok(None);
        }
        let origin = read_point(&mut c)?;
        let x = read_point(&mut c)?;
        let y = read_point(&mut c)?;
        c.take(24 + 32)?; // z axis, plane equation
        let radius = c.f64()?;
        c.take(72)?; // three points on the circle
        let angle = read_interval(&mut c)?;
        let t = read_interval(&mut c)?;
        finite(&[origin.x, origin.y, origin.z, x.x, x.y, x.z, y.x, y.y, y.z, radius, angle[0], angle[1], t[0], t[1]])?;
        return Ok(Some(Curve::Arc { origin, x, y, radius, angle, t }));
    }
    if class == ON_POLYLINE_CURVE {
        c.version()?;
        let n = c.count(24)?;
        let points = (0..n).map(|_| read_point(&mut c)).collect::<R<Vec<_>>>()?;
        let m = c.count(8)?;
        let t = (0..m).map(|_| c.f64()).collect::<R<Vec<_>>>()?;
        if n < 2 || m != n || t.windows(2).any(|w| !(w[1] >= w[0])) || points.iter().any(|p| !p.is_finite()) {
            return Ok(None);
        }
        return Ok(Some(Curve::Polyline { points, t }));
    }
    if ON_NURBS_CURVE.contains(&class) {
        let (major, _) = c.version()?;
        if major != 1 {
            return Ok(None);
        }
        let dim = c.i32()?;
        let rational = c.i32()? != 0;
        let order = c.i32()?.max(0) as usize;
        let count = c.i32()?.max(0) as usize;
        c.take(8 + 48)?; // reserved, bounding box
        if !(1..=3).contains(&dim) || !(2..=MAX_ORDER).contains(&order) || count < order {
            return Ok(None);
        }
        let knot_count = c.count(8)?;
        if knot_count != order + count - 2 {
            return Ok(None);
        }
        let knots = (0..knot_count).map(|_| c.f64()).collect::<R<Vec<_>>>()?;
        let cv_size = dim as usize + rational as usize;
        let n = c.count(8 * cv_size)?;
        if n != count {
            return Ok(None);
        }
        let mut cvs = Vec::with_capacity(count);
        for _ in 0..count {
            let mut h = [0.0, 0.0, 0.0, 1.0];
            for k in 0..dim as usize {
                h[k] = c.f64()?;
            }
            if rational {
                h[3] = c.f64()?;
            }
            cvs.push(h);
        }
        return Ok(Nurbs::new(order, &knots, cvs).ok().map(Curve::Nurbs));
    }
    if ON_POLY_CURVE.contains(&class) {
        c.version()?;
        let count = c.count(4)?;
        c.take(8 + 48)?; // reserved, bounding box
        let n = c.count(8)?;
        let t = (0..n).map(|_| c.f64()).collect::<R<Vec<_>>>()?;
        let mut segments = Vec::with_capacity(count);
        for _ in 0..count {
            match read_curve_object(&mut c, depth + 1)? {
                Some(s) => segments.push(s),
                None => return Ok(None),
            }
        }
        if segments.is_empty() || t.len() != segments.len() + 1 || t.windows(2).any(|w| !(w[1] >= w[0])) {
            return Ok(None);
        }
        return Ok(Some(Curve::Poly { segments, t }));
    }
    Ok(None)
}

// ---------------------------------------------------------------------------------------------
// Surfaces

#[derive(Debug, Clone)]
pub(super) struct NurbsSurface {
    order: [usize; 2],
    knots: [Vec<f64>; 2],
    count: [usize; 2],
    /// Homogeneous control points, `count[1]` per row of u.
    cvs: Vec<[f64; 4]>,
}

impl NurbsSurface {
    fn domain(&self) -> [[f64; 2]; 2] {
        [0, 1].map(|d| [self.knots[d][self.order[d] - 1], self.knots[d][self.count[d]]])
    }

    fn point(&self, u: f64, v: f64) -> DVec3 {
        let [pu, pv] = self.order.map(|o| o - 1);
        let iu = span(&self.knots[0], pu, self.count[0], u);
        let iv = span(&self.knots[1], pv, self.count[1], v);
        let mut rows = [[0.0; 4]; MAX_ORDER];
        let mut d = [[0.0; 4]; MAX_ORDER];
        for a in 0..=pu {
            let row = (iu - pu + a) * self.count[1];
            d[..=pv].copy_from_slice(&self.cvs[row + iv - pv..=row + iv]);
            rows[a] = de_boor(pv, &self.knots[1], iv, &mut d[..=pv], v);
        }
        homogeneous(de_boor(pu, &self.knots[0], iu, &mut rows[..=pu], u))
    }
}

#[derive(Debug, Clone)]
pub(super) enum Surface {
    Plane { origin: DVec3, x: DVec3, y: DVec3, domain: [[f64; 2]; 2], extents: [[f64; 2]; 2] },
    Nurbs(NurbsSurface),
    Revolution { from: DVec3, axis: DVec3, angle: [f64; 2], t: [f64; 2], transposed: bool, curve: Curve },
    Sum { base: DVec3, curves: Box<[Curve; 2]> },
}

impl Surface {
    fn domain(&self) -> [[f64; 2]; 2] {
        match self {
            Surface::Plane { domain, .. } => *domain,
            Surface::Nurbs(n) => n.domain(),
            Surface::Revolution { t, transposed, curve, .. } => {
                if *transposed {
                    [curve.domain(), *t]
                } else {
                    [*t, curve.domain()]
                }
            }
            Surface::Sum { curves, .. } => [curves[0].domain(), curves[1].domain()],
        }
    }

    fn point(&self, u: f64, v: f64) -> DVec3 {
        match self {
            Surface::Plane { origin, x, y, domain, extents } => {
                let s = lerp_param(u, domain[0], extents[0]);
                let t = lerp_param(v, domain[1], extents[1]);
                *origin + s * *x + t * *y
            }
            Surface::Nurbs(n) => n.point(u, v),
            Surface::Revolution { from, axis, angle, t, transposed, curve } => {
                let (a, s) = if *transposed { (v, u) } else { (u, v) };
                let theta = lerp_param(a, *t, *angle);
                let p = curve.point(s) - *from;
                let (sin, cos) = theta.sin_cos();
                *from + p * cos + axis.cross(p) * sin + *axis * axis.dot(p) * (1.0 - cos)
            }
            Surface::Sum { base, curves } => *base + curves[0].point(u) + curves[1].point(v),
        }
    }

    /// Parameters along u (`dir` 0) or v (1) where the surface may have a crease.
    fn kinks(&self, dir: usize) -> Vec<f64> {
        match self {
            Surface::Plane { .. } => Vec::new(),
            Surface::Nurbs(n) => kinks(&n.knots[dir], n.order[dir] - 1, n.domain()[dir]),
            Surface::Revolution { transposed, curve, .. } => {
                if (dir == 1) != *transposed {
                    curve.kinks()
                } else {
                    Vec::new()
                }
            }
            Surface::Sum { curves, .. } => curves[dir].kinks(),
        }
    }

    /// A plane, or a NURBS surface of degree 1 with four control points: lines in its
    /// parameters are straight on it.
    fn is_flat(&self) -> bool {
        match self {
            Surface::Plane { .. } => true,
            Surface::Nurbs(n) => n.order == [2, 2] && n.count == [2, 2] && n.cvs.iter().all(|c| c[3] == 1.0),
            _ => false,
        }
    }

    /// The unit normal `Su × Sv` by central differences, nudged off poles; zero when none.
    fn normal(&self, u: f64, v: f64, dom: [[f64; 2]; 2]) -> DVec3 {
        let du = 1e-6 * (dom[0][1] - dom[0][0]);
        let dv = 1e-6 * (dom[1][1] - dom[1][0]);
        let at = |u: f64, v: f64| {
            let (u0, u1) = ((u - du).max(dom[0][0]), (u + du).min(dom[0][1]));
            let (v0, v1) = ((v - dv).max(dom[1][0]), (v + dv).min(dom[1][1]));
            let su = self.point(u1, v) - self.point(u0, v);
            let sv = self.point(u, v1) - self.point(u, v0);
            su.cross(sv).normalize_or_zero()
        };
        let n = at(u, v);
        if n != DVec3::ZERO {
            return n;
        }
        // a pole (sphere tip, cone apex): the normal next to it
        let (cu, cv) = (0.5 * (dom[0][0] + dom[0][1]), 0.5 * (dom[1][0] + dom[1][1]));
        at(u + 1e-4 * (cu - u), v + 1e-4 * (cv - v))
    }
}

fn read_surface_object(c: &mut Cur<'_>) -> R<Option<Surface>> {
    let Some(o) = c.object()? else { return Ok(None) };
    read_surface(o.class, o.data)
}

/// Whether objects of this class are surfaces the viewer can evaluate.
pub(super) fn is_surface(class: Uuid) -> bool {
    class == ON_PLANE_SURFACE || ON_NURBS_SURFACE.contains(&class) || ON_REV_SURFACE.contains(&class) || ON_SUM_SURFACE.contains(&class)
}

/// A surface object of its own (not the face of a polysurface), as programs writing 3dm files
/// with openNURBS store one: meshed over its whole domain.
pub(super) fn surface_object(class: Uuid, data: Cur<'_>, vertices: &mut usize) -> R<Option<Mesh>> {
    let Some(surface) = read_surface(class, data)? else { return Ok(None) };
    let Some(mesh) = face_mesh(&surface, Vec::new(), false) else { return Ok(None) };
    if mesh.positions.len() > super::MAX_VERTICES - *vertices {
        return Err(super::TOO_MANY_VERTICES.into());
    }
    *vertices += mesh.positions.len();
    Ok(Some(mesh))
}

fn read_surface(class: Uuid, mut c: Cur<'_>) -> R<Option<Surface>> {
    if class == ON_PLANE_SURFACE {
        let (major, minor) = c.version()?;
        if major != 1 {
            return Ok(None);
        }
        let origin = read_point(&mut c)?;
        let x = read_point(&mut c)?;
        let y = read_point(&mut c)?;
        c.take(24 + 32)?; // z axis, plane equation
        let domain = [read_interval(&mut c)?, read_interval(&mut c)?];
        let extents = if minor >= 1 { [read_interval(&mut c)?, read_interval(&mut c)?] } else { domain };
        finite(&[origin.x, origin.y, origin.z, x.x, x.y, x.z, y.x, y.y, y.z])?;
        finite(domain.as_flattened())?;
        finite(extents.as_flattened())?;
        return Ok(Some(Surface::Plane { origin, x, y, domain, extents }));
    }
    if ON_NURBS_SURFACE.contains(&class) {
        let (major, _) = c.version()?;
        if major != 1 {
            return Ok(None);
        }
        let dim = c.i32()?;
        let rational = c.i32()? != 0;
        let order = [c.i32()?.max(0) as usize, c.i32()?.max(0) as usize];
        let count = [c.i32()?.max(0) as usize, c.i32()?.max(0) as usize];
        c.take(8 + 48)?; // reserved, bounding box
        if !(1..=3).contains(&dim) || order.iter().any(|o| !(2..=MAX_ORDER).contains(o)) || count[0] < order[0] || count[1] < order[1] {
            return Ok(None);
        }
        let mut knots: [Vec<f64>; 2] = Default::default();
        for d in 0..2 {
            let n = c.count(8)?;
            if n != order[d] + count[d] - 2 {
                return Ok(None);
            }
            let k = (0..n).map(|_| c.f64()).collect::<R<Vec<_>>>()?;
            let mut full = Vec::with_capacity(n + 2);
            full.push(k[0]);
            full.extend_from_slice(&k);
            full.push(k[n - 1]);
            if full.windows(2).any(|w| !(w[1] >= w[0])) {
                return Ok(None);
            }
            knots[d] = full;
        }
        let cv_size = dim as usize + rational as usize;
        let total = c.count(8 * cv_size)?;
        if Some(total) != count[0].checked_mul(count[1]) {
            return Ok(None);
        }
        let mut cvs = Vec::with_capacity(total);
        for _ in 0..total {
            let mut h = [0.0, 0.0, 0.0, 1.0];
            for k in 0..dim as usize {
                h[k] = c.f64()?;
            }
            if rational {
                h[3] = c.f64()?;
            }
            cvs.push(h);
        }
        finite(cvs.as_flattened())?;
        let s = NurbsSurface { order, knots, count, cvs };
        let dom = s.domain();
        if dom.iter().any(|d| !(d[1] > d[0])) {
            return Ok(None);
        }
        return Ok(Some(Surface::Nurbs(s)));
    }
    if ON_REV_SURFACE.contains(&class) {
        let (major, _) = c.version()?;
        if major != 1 && major != 2 {
            return Ok(None);
        }
        let from = read_point(&mut c)?;
        let to = read_point(&mut c)?;
        let angle = read_interval(&mut c)?;
        let t = if major == 2 { read_interval(&mut c)? } else { [angle[0].min(angle[1]), angle[0].max(angle[1])] };
        c.take(48)?; // bounding box
        let transposed = c.i32()? != 0;
        if !c.bool()? {
            return Ok(None);
        }
        let Some(curve) = read_curve_object(&mut c, 0)? else { return Ok(None) };
        let axis = (to - from).normalize_or_zero();
        finite(&[from.x, from.y, from.z, angle[0], angle[1], t[0], t[1]])?;
        if axis == DVec3::ZERO || !(t[1] > t[0]) {
            return Ok(None);
        }
        return Ok(Some(Surface::Revolution { from, axis, angle, t, transposed, curve }));
    }
    if ON_SUM_SURFACE.contains(&class) {
        c.version()?;
        let base = read_point(&mut c)?;
        c.take(48)?; // bounding box
        let (Some(a), Some(b)) = (read_curve_object(&mut c, 0)?, read_curve_object(&mut c, 0)?) else { return Ok(None) };
        finite(&[base.x, base.y, base.z])?;
        return Ok(Some(Surface::Sum { base, curves: Box::new([a, b]) }));
    }
    Ok(None)
}

// ---------------------------------------------------------------------------------------------
// Polysurfaces

struct Trim {
    curve: i32,
    /// The part of the curve the trim uses, and whether it runs backwards.
    proxy: [f64; 2],
    reversed: bool,
}

struct Loop {
    trims: Vec<i32>,
    /// 1 outer, 2 inner, 3 slit.
    kind: i32,
}

struct Face {
    loops: Vec<i32>,
    surface: i32,
    reversed: bool,
}

/// `ON_CurveArray` / `ON_SurfaceArray`: a version, a count and, per item, a flag and an object.
fn read_array<T>(mut c: Cur<'_>, mut read: impl FnMut(&mut Cur<'_>) -> R<Option<T>>) -> R<Vec<Option<T>>> {
    c.version()?;
    let count = c.count(4)?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(if c.i32()? == 1 { read(&mut c).unwrap_or(None) } else { None });
    }
    Ok(out)
}

fn read_ints(c: &mut Cur<'_>) -> R<Vec<i32>> {
    let n = c.count(4)?;
    (0..n).map(|_| c.i32()).collect()
}

fn read_trims(mut c: Cur<'_>) -> R<Vec<Trim>> {
    let (major, _) = c.version()?;
    if major != 1 {
        return err("trims of an unknown version");
    }
    let new = c.v.archive >= 3 && c.v.opennurbs >= 200_206_180;
    let count = c.count(72)?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        c.i32()?; // index
        let curve = c.i32()?;
        let proxy = read_interval(&mut c)?;
        c.take(4 + 8 + 4 + 4 + 4 + 4 + 16)?; // edge, vertices, reversed 3d, type, iso, loop, tolerances
        let mut reversed = false;
        if new {
            read_interval(&mut c)?; // domain
            reversed = c.take(8)?[0] == 1;
            c.take(24)?;
        } else {
            c.take(48)?;
        }
        c.take(16)?; // legacy tolerances
        out.push(Trim { curve, proxy, reversed });
    }
    Ok(out)
}

fn read_loops(mut c: Cur<'_>) -> R<Vec<Loop>> {
    c.version()?;
    let count = c.count(16)?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        c.i32()?;
        let trims = read_ints(&mut c)?;
        let kind = c.i32()?;
        c.i32()?; // face
        out.push(Loop { trims, kind });
    }
    Ok(out)
}

fn read_faces(mut c: Cur<'_>) -> R<Vec<Face>> {
    c.version()?;
    let count = c.count(20)?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        c.i32()?;
        let loops = read_ints(&mut c)?;
        let surface = c.i32()?;
        let reversed = c.i32()? != 0;
        c.i32()?; // material channel
        out.push(Face { loops, surface, reversed });
    }
    Ok(out)
}

/// Meshes of the faces of an `ON_Brep` from its chunks (2d curves, 3d curves, surfaces,
/// vertices, edges, trims, loops, faces), one per face, with the face colours. Faces that cannot
/// be meshed are left out. `vertices` counts the mesh vertices read or made so far.
pub(super) fn brep(parts: &[Cur<'_>; 8], size: f64, colors: &[Option<[u8; 4]>], vertices: &mut usize) -> R<Vec<Mesh>> {
    let curves = read_array(parts[0].clone(), |c| read_curve_object(c, 0))?;
    let surfaces = read_array(parts[2].clone(), read_surface_object)?;
    let trims = read_trims(parts[5].clone())?;
    let loops = read_loops(parts[6].clone())?;
    let faces = read_faces(parts[7].clone())?;
    let tol = tolerance(size);
    let mut out = Vec::new();
    for (k, face) in faces.iter().enumerate() {
        let Some(Some(surface)) = usize::try_from(face.surface).ok().and_then(|s| surfaces.get(s)) else { continue };
        let mut rings = Vec::new();
        let mut complete = true;
        let bend = if surface.is_flat() { 0 } else { 2 };
        let cell = grid_cell(surface);
        for &l in &face.loops {
            let Some(lp) = usize::try_from(l).ok().and_then(|l| loops.get(l)) else { continue };
            if lp.kind == 3 {
                continue; // a slit has no inside
            }
            let mut ring = Vec::new();
            for &t in &lp.trims {
                let trim = usize::try_from(t).ok().and_then(|t| trims.get(t));
                let curve = trim.and_then(|t| usize::try_from(t.curve).ok()).and_then(|c| curves.get(c)).and_then(Option::as_ref);
                let (Some(trim), Some(curve)) = (trim, curve) else {
                    complete = false;
                    break;
                };
                let [a, b] = if trim.reversed { [trim.proxy[1], trim.proxy[0]] } else { trim.proxy };
                let map = |p: DVec3| surface.point(p.x, p.y);
                let params = curve.sample(a, b, tol, &map, bend, cell);
                ring.extend(params[..params.len() - 1].iter().map(|&s| curve.point(s).truncate()));
            }
            if !complete {
                break;
            }
            if ring.len() >= 3 {
                rings.push(ring);
            }
        }
        if !complete {
            continue;
        }
        let Some(mut mesh) = face_mesh(surface, rings, face.reversed) else { continue };
        if mesh.positions.len() > super::MAX_VERTICES - *vertices {
            return Err(super::TOO_MANY_VERTICES.into());
        }
        *vertices += mesh.positions.len();
        mesh.color = colors.get(k).copied().flatten();
        out.push(mesh);
    }
    Ok(out)
}

/// The absolute chord tolerance of an object of the given size (bounding box diagonal).
fn tolerance(size: f64) -> f64 {
    if size.is_finite() && size > 0.0 {
        size * 2e-4
    } else {
        1e-3
    }
}

/// Twice the signed area of a ring.
fn area(ring: &[DVec2]) -> f64 {
    (0..ring.len()).map(|i| ring[i].perp_dot(ring[(i + 1) % ring.len()])).sum()
}

fn inside(rings: &[Vec<DVec2>], p: DVec2) -> bool {
    let mut odd = false;
    for ring in rings {
        let n = ring.len();
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                odd = !odd;
            }
        }
    }
    odd
}

fn distance_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let len = ab.length_squared();
    let t = if len > 0.0 { ((p - a).dot(ab) / len).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + t * ab)).length()
}

/// The size of a grid cell of a curved surface in its parameters, over its whole domain; `None`
/// for a flat one.
fn grid_cell(surface: &Surface) -> Option<DVec2> {
    if surface.is_flat() {
        return None;
    }
    let d = surface.domain();
    if d.iter().any(|d| !(d[1] > d[0])) || d.as_flattened().iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some(DVec2::new((d[0][1] - d[0][0]) / divisions(surface, 0, d, d) as f64, (d[1][1] - d[1][0]) / divisions(surface, 1, d, d) as f64))
}

/// How many parts the surface bends into along u (`dir` 0) or v (1) over `range`, so that a part
/// turns at most [`MAX_ANGLE`].
fn divisions(surface: &Surface, dir: usize, range: [[f64; 2]; 2], dom: [[f64; 2]; 2]) -> usize {
    const SAMPLES: usize = 32;
    let other = 1 - dir;
    let mut worst: f64 = 0.0;
    // evenly, and just before and after every kink, so that a crease is not stepped over
    let [lo, hi] = range[dir];
    let mut params: Vec<f64> = (0..=SAMPLES).map(|i| lo + (hi - lo) * i as f64 / SAMPLES as f64).collect();
    let eps = 1e-6 * (hi - lo);
    for k in surface.kinks(dir).into_iter().filter(|&k| k > lo && k < hi) {
        params.extend([k - eps, k + eps]);
    }
    params.sort_by(f64::total_cmp);
    for k in 0..3 {
        let w = range[other][0] + (range[other][1] - range[other][0]) * (0.1 + 0.4 * k as f64);
        let uv = |s: f64| if dir == 0 { (s, w) } else { (w, s) };
        let mut turn = 0.0;
        let mut bend = 0.0;
        let mut twist = 0.0;
        // the surface's direction across: it turns along a twisted (ruled) surface
        let h = 1e-6 * (dom[other][1] - dom[other][0]);
        let across = |u: f64, v: f64| {
            let (a, b) = if dir == 0 { ((u, (v - h).max(dom[1][0])), (u, (v + h).min(dom[1][1]))) } else { (((u - h).max(dom[0][0]), v), ((u + h).min(dom[0][1]), v)) };
            (surface.point(b.0, b.1) - surface.point(a.0, a.1)).normalize_or_zero()
        };
        let mut last: Option<(DVec3, DVec3, DVec3, DVec3)> = None; // point, direction, normal, across
        for &s in &params {
            let (u, v) = uv(s);
            let p = surface.point(u, v);
            let n = surface.normal(u, v, dom);
            let x = across(u, v);
            if let Some((q, d, m, y)) = last {
                let e = (p - q).normalize_or_zero();
                if d != DVec3::ZERO && e != DVec3::ZERO {
                    turn += d.angle_between(e);
                }
                if m != DVec3::ZERO && n != DVec3::ZERO {
                    bend += m.angle_between(n);
                }
                if y != DVec3::ZERO && x != DVec3::ZERO {
                    twist += y.angle_between(x);
                }
                last = Some((p, if e == DVec3::ZERO { d } else { e }, n, x));
            } else {
                last = Some((p, DVec3::ZERO, n, x));
            }
        }
        if turn.is_finite() && bend.is_finite() && twist.is_finite() {
            worst = worst.max(turn).max(bend).max(twist);
        }
    }
    ((worst / MAX_ANGLE).ceil() as usize).clamp(1, MAX_GRID)
}

/// Whether a loop runs along the edges of the domain only: the face is untrimmed.
fn on_domain_edges(ring: &[DVec2], dom: [[f64; 2]; 2]) -> bool {
    let e = [1e-9 * (dom[0][1] - dom[0][0]), 1e-9 * (dom[1][1] - dom[1][0])];
    let near = |a: f64, b: f64, e: f64| (a - b).abs() <= e;
    let mut corners = [false; 4];
    for p in ring {
        let (u0, u1, v0, v1) = (near(p.x, dom[0][0], e[0]), near(p.x, dom[0][1], e[0]), near(p.y, dom[1][0], e[1]), near(p.y, dom[1][1], e[1]));
        if !(u0 || u1 || v0 || v1) {
            return false;
        }
        for (k, c) in [(u0 && v0), (u1 && v0), (u1 && v1), (u0 && v1)].into_iter().enumerate() {
            corners[k] |= c;
        }
    }
    corners.iter().all(|&c| c)
}

/// The triangles of an untrimmed face: a grid over the domain, its lines spaced so that the
/// surface turns at most [`MAX_ANGLE`] between them and laid on its kinks; each cell two
/// triangles across its shorter diagonal. The edge points are the grid's, so no long triangle
/// zigzags over the bending.
fn grid_mesh(surface: &Surface, dom: [[f64; 2]; 2], reversed: bool) -> Option<Mesh> {
    let lines = |dir: usize| {
        let [a, b] = dom[dir];
        let n = divisions(surface, dir, dom, dom);
        let mut out: Vec<f64> = (0..=n).map(|i| a + (b - a) * i as f64 / n as f64).collect();
        out.extend(surface.kinks(dir).into_iter().filter(|&k| k > a && k < b));
        out.sort_by(f64::total_cmp);
        out.dedup_by(|x, y| (*x - *y).abs() <= 1e-9 * (b - a));
        out
    };
    let (us, vs) = (lines(0), lines(1));
    let (nu, nv) = (us.len(), vs.len());
    let mut points = Vec::with_capacity(nu * nv);
    let mut normals = Vec::with_capacity(nu * nv);
    for &u in &us {
        for &v in &vs {
            points.push(surface.point(u, v));
            let n = surface.normal(u, v, dom);
            normals.push(if reversed { -n } else { n });
        }
    }
    if points.iter().any(|p| !p.is_finite()) {
        return None;
    }
    let at = |i: usize, j: usize| (i * nv + j) as u32;
    let mut mesh = Mesh::default();
    for i in 0..nu - 1 {
        for j in 0..nv - 1 {
            // counterclockwise in the parameter plane, along Su x Sv
            let q = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
            let d = |a: u32, b: u32| (points[a as usize] - points[b as usize]).length_squared();
            let tris = if d(q[0], q[2]) <= d(q[1], q[3]) { [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] } else { [[q[0], q[1], q[3]], [q[1], q[2], q[3]]] };
            for t in tris {
                let p = t.map(|k| points[k as usize]);
                if (p[1] - p[0]).cross(p[2] - p[0]).length_squared() == 0.0 {
                    continue; // at a pole
                }
                let t = if reversed { [t[0], t[2], t[1]] } else { t };
                mesh.faces.push([t[0], t[1], t[2], t[2]]);
            }
        }
    }
    if mesh.faces.is_empty() {
        return None;
    }
    mesh.triangles = mesh.faces.len();
    mesh.positions = points.iter().map(|p| p.to_array().map(|v| v as f32)).collect();
    mesh.normals = Some(normals.iter().map(|n| n.to_array().map(|v| v as f32)).collect());
    mesh.computed = true;
    Some(mesh)
}

/// The triangles of a face: its loops (in the surface's parameters) triangulated with the grid
/// points inside them, on the surface.
fn face_mesh(surface: &Surface, rings: Vec<Vec<DVec2>>, reversed: bool) -> Option<Mesh> {
    let dom = surface.domain();
    if dom.as_flattened().iter().any(|v| !v.is_finite()) || dom.iter().any(|d| !(d[1] > d[0])) {
        return None;
    }
    if rings.is_empty() || (rings.len() == 1 && on_domain_edges(&rings[0], dom)) {
        // an untrimmed face: a grid over the whole domain
        return grid_mesh(surface, dom, reversed);
    }
    // the loops' box in the parameter plane
    let mut lo = DVec2::splat(f64::INFINITY);
    let mut hi = DVec2::splat(f64::NEG_INFINITY);
    for p in rings.iter().flatten() {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    if !lo.is_finite() || !hi.is_finite() || hi.x <= lo.x || hi.y <= lo.y {
        return None;
    }
    let range = [[lo.x, hi.x], [lo.y, hi.y]];
    // How many parts the surface bends into each way. A curved face is triangulated with one
    // grid cell per unit, so that its triangles follow the bending (a thin pipe's run along it,
    // never across it); a flat one with its parameters scaled to lengths on the surface.
    let mut n = [divisions(surface, 0, range, dom), divisions(surface, 1, range, dom)];
    // at least as fine as the cells its trims were sampled by
    if let Some(cell) = grid_cell(surface) {
        n[0] = n[0].max((((hi.x - lo.x) / cell.x).ceil() as usize).clamp(1, MAX_GRID));
        n[1] = n[1].max((((hi.y - lo.y) / cell.y).ceil() as usize).clamp(1, MAX_GRID));
    }
    let curved = n[0] > 1 || n[1] > 1;
    let scale = if curved {
        // a straight direction keeps one cell: its triangles run from edge to edge, as a
        // cylinder's should (a row in between, about as high as the circle's sag, would tilt
        // them into a Schwarz lantern and add area)
        [n[0] as f64 / (hi.x - lo.x), n[1] as f64 / (hi.y - lo.y)]
    } else {
        let mut scale = [0.0; 2];
        for i in 0..3 {
            for j in 0..3 {
                let u = lo.x + (hi.x - lo.x) * (0.1 + 0.4 * i as f64);
                let v = lo.y + (hi.y - lo.y) * (0.1 + 0.4 * j as f64);
                let p = surface.point(u, v);
                let du = 1e-3 * (hi.x - lo.x);
                let dv = 1e-3 * (hi.y - lo.y);
                scale[0] += (surface.point(u + du, v) - p).length() / du;
                scale[1] += (surface.point(u, v + dv) - p).length() / dv;
            }
        }
        scale.map(|s| if s.is_finite() && s > 0.0 { s / 9.0 } else { 1.0 })
    };
    let to_plane = |p: DVec2| DVec2::new((p.x - lo.x) * scale[0], (p.y - lo.y) * scale[1]);
    let scaled: Vec<Vec<DVec2>> = rings.iter().map(|r| r.iter().map(|&p| to_plane(p)).collect()).collect();

    let mut cdt = ConstrainedDelaunayTriangulation::<Point2<f64>>::new();
    for ring in &scaled {
        let handles: Vec<_> = ring.iter().filter_map(|p| cdt.insert(Point2::new(p.x, p.y)).ok()).collect();
        for k in 0..handles.len() {
            let (a, b) = (handles[k], handles[(k + 1) % handles.len()]);
            if a != b {
                cdt.try_add_constraint(a, b);
            }
        }
    }
    // the grid points inside the loops, away from them
    if curved {
        let segments: Vec<(DVec2, DVec2)> = scaled.iter().flat_map(|r| (0..r.len()).map(move |k| (r[k], r[(k + 1) % r.len()]))).collect();
        // grid lines: evenly, and on the kinks of the surface so that no triangle cuts a crease
        let lines = |dir: usize| {
            let (l, sc) = if dir == 0 { (lo.x, scale[0]) } else { (lo.y, scale[1]) };
            let mut out: Vec<f64> = (1..n[dir]).map(|i| i as f64).collect();
            out.extend(surface.kinks(dir).into_iter().map(|k| (k - l) * sc).filter(|&g| g > 0.0 && g < n[dir] as f64));
            out.sort_by(f64::total_cmp);
            out.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            out
        };
        let (us, vs) = (lines(0), lines(1));
        for &i in &us {
            for &j in &vs {
                let p = DVec2::new(i, j);
                if inside(&scaled, p) && segments.iter().all(|&(a, b)| distance_to_segment(p, a, b) > 0.3) {
                    let _ = cdt.insert(Point2::new(p.x, p.y));
                }
            }
        }
    }

    let points: Vec<DVec2> = cdt.vertices().map(|v| DVec2::new(v.position().x, v.position().y)).collect();
    let mut triangles = Vec::new();
    for f in cdt.inner_faces() {
        let [a, b, c] = f.vertices().map(|v| v.fix().index());
        let (pa, pb, pc) = (points[a], points[b], points[c]);
        let twice = (pb - pa).perp_dot(pc - pa);
        // a sliver along a boundary of almost collinear points (rounding): no area on the
        // surface, but on a curved one its corners can lie far apart
        let longest = (pb - pa).length_squared().max((pc - pb).length_squared()).max((pa - pc).length_squared());
        if twice.abs() <= 1e-9 * longest || !inside(&scaled, (pa + pb + pc) / 3.0) {
            continue;
        }
        // counterclockwise in the parameter plane is along Su × Sv
        let t = if (twice > 0.0) != reversed { [a, b, c] } else { [a, c, b] };
        triangles.push(t);
    }
    if triangles.is_empty() {
        return None;
    }
    // only the vertices used, on the surface
    let mut index = vec![u32::MAX; points.len()];
    let mut mesh = Mesh::default();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    for t in &mut triangles {
        for i in t.iter_mut() {
            if index[*i] == u32::MAX {
                index[*i] = positions.len() as u32;
                let uv = DVec2::new(points[*i].x / scale[0] + lo.x, points[*i].y / scale[1] + lo.y);
                let p = surface.point(uv.x, uv.y);
                let normal = surface.normal(uv.x, uv.y, dom);
                positions.push(p.to_array().map(|v| v as f32));
                normals.push((if reversed { -normal } else { normal }).to_array().map(|v| v as f32));
            }
            *i = index[*i] as usize;
        }
        mesh.faces.push([t[0] as u32, t[1] as u32, t[2] as u32, t[2] as u32]);
    }
    if positions.iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    mesh.triangles = mesh.faces.len();
    mesh.positions = positions;
    mesh.normals = Some(normals);
    mesh.computed = true;
    Some(mesh)
}

// ---------------------------------------------------------------------------------------------
// Extrusions

/// An `ON_Extrusion`'s shape: the profile (a polycurve of several profiles when there are
/// holes), the path and which ends are capped.
pub(super) struct Extrusion {
    pub(super) profile: Option<Curve>,
    pub(super) from: DVec3,
    pub(super) to: DVec3,
    pub(super) t: [f64; 2],
    pub(super) up: DVec3,
    pub(super) profiles: i32,
    pub(super) caps: [bool; 2],
    /// The normals of the miter planes at the ends, in the profile's plane coordinates.
    pub(super) miters: [Option<DVec3>; 2],
}

/// Reads the shape of an extrusion from its chunk, after the profile; returns the minor version.
pub(super) fn read_extrusion_shape(b: &mut Cur<'_>, profile: Option<Curve>, minor: i32) -> R<(Extrusion, i32)> {
    let from = read_point(b)?;
    let to = read_point(b)?;
    let t = read_interval(b)?;
    let up = read_point(b)?;
    let have = [b.bool()?, b.bool()?];
    let normals = [read_point(b)?, read_point(b)?];
    b.take(16 + 1)?; // path domain, transposed
    let miters = [0, 1].map(|k| Some(normals[k]).filter(|_| have[k]));
    let mut e = Extrusion { profile, from, to, t, up, profiles: 1, caps: [false; 2], miters };
    if minor >= 1 {
        e.profiles = b.i32()?;
        if minor >= 2 {
            e.caps = [b.bool()?, b.bool()?];
        }
    }
    Ok((e, minor))
}

/// The axis and angle (sine, cosine) that turn the profile plane into a miter plane of normal
/// `n`; `None` when `n` leaves it as it is or is not valid (`ON_Extrusion::m_Nz_min`).
fn miter_plane(n: DVec3) -> Option<(DVec3, f64, f64)> {
    if !(n.z > 1.0 / 64.0) || (n.length() - 1.0).abs() > 1e-6 || (n.x == 0.0 && n.y == 0.0) {
        return None;
    }
    let a = DVec3::new(-n.y, n.x, 0.0);
    let sin = a.length();
    Some((a / sin, sin, n.z))
}

/// A profile point in the miter plane of normal `n` (in the profile's coordinates): scaled
/// across the tilt so that its projection on the profile plane stays where it was, then turned
/// into the plane.
fn miter(n: Option<DVec3>, q: DVec2) -> DVec3 {
    let Some((a, sin, cos)) = n.and_then(miter_plane) else { return q.extend(0.0) };
    let c = 1.0 - 1.0 / cos;
    let v = DVec3::new((1.0 - c * a.y * a.y) * q.x + c * a.x * a.y * q.y, c * a.x * a.y * q.x + (1.0 - c * a.x * a.x) * q.y, 0.0);
    v * cos + a.cross(v) * sin + a * a.dot(v) * (1.0 - cos)
}

/// The mesh of an extrusion: walls along its profiles and its caps. `None` when it has no
/// profile or a degenerate path.
pub(super) fn extrusion(e: &Extrusion, minor: i32, vertices: &mut usize) -> R<Option<Mesh>> {
    let Some(profile) = &e.profile else { return Ok(None) };
    let axis = e.to - e.from;
    let z = axis.normalize_or_zero();
    let y = (e.up - z * e.up.dot(z)).normalize_or_zero();
    if z == DVec3::ZERO || y == DVec3::ZERO || !e.t[0].is_finite() || !e.t[1].is_finite() {
        return Ok(None);
    }
    let x = y.cross(z);
    let bottom = e.from + e.t[0] * axis;
    let top = e.from + e.t[1] * axis;
    let profiles: Vec<&Curve> = match profile {
        Curve::Poly { segments, .. } if e.profiles > 1 => segments.iter().collect(),
        c => vec![c],
    };
    // the profiles as polygons in their plane
    let mut size: f64 = 0.0;
    for p in &profiles {
        let [a, b] = p.domain();
        for k in 0..=8 {
            size = size.max(p.point(a + (b - a) * k as f64 / 8.0).truncate().length());
        }
    }
    let tol = tolerance(size.max(1e-9) * 2.0);
    let mut polygons: Vec<(Vec<DVec2>, bool)> = Vec::new();
    for p in &profiles {
        let [a, b] = p.domain();
        let params = p.sample(a, b, tol, &|q| q, 0, None);
        let mut ring: Vec<DVec2> = params.iter().map(|&s| p.point(s).truncate()).collect();
        let closed = ring.len() > 2 && ring[0].distance(ring[ring.len() - 1]) <= 1e-9 * size.max(1.0);
        if closed {
            ring.pop();
        }
        if ring.len() >= 2 && ring.iter().all(|q| q.is_finite()) {
            polygons.push((ring, closed));
        }
    }
    if polygons.is_empty() {
        return Ok(None);
    }
    let caps = if minor >= 2 { e.caps } else { [polygons[0].1; 2] };
    // a profile point at an end: in the end's miter plane when it has one
    // (`ON_GetEndCapTransformation`), its projection along the path unchanged
    let ends = [bottom, top];
    let place = |end: usize, q: DVec2| {
        let l = miter(e.miters[end], q);
        ends[end] + l.x * x + l.y * y + l.z * z
    };
    let end_normal = |end: usize| {
        let n = e.miters[end].filter(|n| miter_plane(*n).is_some()).unwrap_or(DVec3::Z);
        (n.x * x + n.y * y + n.z * z).normalize_or_zero()
    };
    let mut positions: Vec<DVec3> = Vec::new();
    let mut normals: Vec<DVec3> = Vec::new();
    let mut faces = Vec::new();
    for (k, (ring, closed)) in polygons.iter().enumerate() {
        // walls face away from the material: outward of the outer profile, into the holes
        let ccw = area(ring) > 0.0;
        let outward = if !*closed { 1.0 } else if (k == 0) == ccw { 1.0 } else { -1.0 };
        let n = ring.len();
        let segs = if *closed { n } else { n - 1 };
        let normal_of = |s: usize| {
            let d = ring[(s + 1) % n] - ring[s];
            (d.y * x - d.x * y).normalize_or_zero() * outward
        };
        for s in 0..segs {
            let (i, j) = (s, (s + 1) % n);
            let here = normal_of(s);
            // smooth across a vertex that turns less than about 30 degrees
            let smooth = |m: DVec3| if m != DVec3::ZERO && here.angle_between(m) < 0.5 { (here + m).normalize_or_zero() } else { here };
            let ni = if *closed || s > 0 { smooth(normal_of((s + segs - 1) % segs)) } else { here };
            let nj = if *closed || s + 1 < segs { smooth(normal_of((s + 1) % segs)) } else { here };
            let base = positions.len() as u32;
            positions.extend([place(0, ring[i]), place(0, ring[j]), place(1, ring[j]), place(1, ring[i])]);
            normals.extend([ni, nj, nj, ni]);
            if outward > 0.0 {
                faces.push([base, base + 1, base + 2, base + 3]);
            } else {
                faces.push([base, base + 3, base + 2, base + 1]);
            }
        }
    }
    // flat caps on closed profiles
    let closed: Vec<&Vec<DVec2>> = polygons.iter().filter(|(_, c)| *c).map(|(r, _)| r).collect();
    if !closed.is_empty() && (caps[0] || caps[1]) {
        let mut flat = Vec::new();
        let mut holes = Vec::new();
        for (k, ring) in closed.iter().enumerate() {
            if k > 0 {
                holes.push(flat.len() / 2);
            }
            flat.extend(ring.iter().flat_map(|q| [q.x, q.y]));
        }
        let corners: Vec<DVec2> = closed.iter().flat_map(|r| r.iter().copied()).collect();
        if let Ok(tris) = earcutr::earcut(&flat, &holes, 2) {
            for (end, normal) in [(0, -end_normal(0)), (1, end_normal(1))] {
                if !caps[end] {
                    continue;
                }
                let base = positions.len() as u32;
                positions.extend(corners.iter().map(|&q| place(end, q)));
                normals.extend(std::iter::repeat_n(normal, corners.len()));
                for t in tris.chunks_exact(3) {
                    let (a, b, c) = (t[0] as u32, t[1] as u32, t[2] as u32);
                    let ccw = (corners[t[1]] - corners[t[0]]).perp_dot(corners[t[2]] - corners[t[0]]) > 0.0;
                    let up = end == 1;
                    faces.push(if ccw == up { [base + a, base + b, base + c, base + c] } else { [base + a, base + c, base + b, base + b] });
                }
            }
        }
    }
    if faces.is_empty() || positions.iter().any(|p| !p.is_finite()) {
        return Ok(None);
    }
    if positions.len() > super::MAX_VERTICES - *vertices {
        return Err(super::TOO_MANY_VERTICES.into());
    }
    *vertices += positions.len();
    let triangles = faces.iter().map(|f| if f[2] == f[3] { 1 } else { 2 }).sum();
    Ok(Some(Mesh {
        positions: positions.iter().map(|p| p.to_array().map(|v| v as f32)).collect(),
        normals: Some(normals.iter().map(|n| n.to_array().map(|v| v as f32)).collect()),
        colors: None,
        faces,
        triangles,
        color: None,
        computed: true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quarter circle as a rational quadratic NURBS curve.
    fn quarter() -> Nurbs {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        Nurbs::new(3, &[0.0, 0.0, 1.0, 1.0], vec![[1.0, 0.0, 0.0, 1.0], [w, w, 0.0, w], [0.0, 1.0, 0.0, 1.0]]).unwrap()
    }

    #[test]
    fn rational_nurbs_curves_are_exact() {
        let q = quarter();
        for k in 0..=10 {
            let p = q.point(k as f64 / 10.0);
            assert!((p.length() - 1.0).abs() < 1e-12, "{p}");
        }
        assert_eq!(q.point(0.0), DVec3::X);
        assert!((q.point(1.0) - DVec3::Y).length() < 1e-12);
    }

    #[test]
    fn sampling_follows_the_tolerance() {
        let c = Curve::Nurbs(quarter());
        let params = c.sample(0.0, 1.0, 1e-4, &|p| p, 0, None);
        // a quarter circle in parts that turn at most about 9 degrees
        assert!(params.len() >= 10 && params.len() < 40, "{}", params.len());
        let back = c.sample(1.0, 0.0, 1e-4, &|p| p, 0, None);
        assert_eq!(back.first(), Some(&1.0));
        assert_eq!(back.last(), Some(&0.0));
    }

    #[test]
    fn a_trimmed_plane_with_a_hole() {
        let plane = Surface::Plane { origin: DVec3::ZERO, x: DVec3::X, y: DVec3::Y, domain: [[0.0, 10.0], [0.0, 10.0]], extents: [[0.0, 10.0], [0.0, 10.0]] };
        let outer = vec![DVec2::new(0.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(10.0, 10.0), DVec2::new(0.0, 10.0)];
        let hole = vec![DVec2::new(4.0, 4.0), DVec2::new(4.0, 6.0), DVec2::new(6.0, 6.0), DVec2::new(6.0, 4.0)];
        let mesh = face_mesh(&plane, vec![outer, hole], false).unwrap();
        // the area of the square less the hole
        let p = |i: u32| DVec3::from(mesh.positions[i as usize].map(f64::from));
        let total: f64 = mesh.faces.iter().map(|f| 0.5 * (p(f[1]) - p(f[0])).cross(p(f[2]) - p(f[0])).z).sum();
        assert!((total - 96.0).abs() < 1e-9, "{total}");
    }

    #[test]
    fn a_miter_tilts_the_end_and_keeps_the_footprint() {
        // a 1 by 1 square tube, 2 high, its top cut at 45 degrees
        let square = Curve::Polyline { points: [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)].map(|(x, y)| DVec3::new(x, y, 0.0)).to_vec(), t: vec![0.0, 1.0, 2.0, 3.0, 4.0] };
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let e = Extrusion { profile: Some(square), from: DVec3::ZERO, to: DVec3::new(0.0, 0.0, 2.0), t: [0.0, 1.0], up: DVec3::Y, profiles: 1, caps: [true, true], miters: [None, Some(DVec3::new(0.0, -h, h))] };
        let mut vertices = 0;
        let mesh = extrusion(&e, 2, &mut vertices).unwrap().unwrap();
        let p = |i: u32| DVec3::from(mesh.positions[i as usize].map(f64::from));
        let tri = |f: &[u32; 4]| 0.5 * (p(f[1]) - p(f[0])).cross(p(f[2]) - p(f[0])).length();
        let total: f64 = mesh.faces.iter().map(|f| if f[2] == f[3] { tri(f) } else { tri(f) + 0.5 * (p(f[2]) - p(f[0])).cross(p(f[3]) - p(f[0])).length() }).sum();
        // the miter plane passes through the path's end at the profile's corner (0, 0): the top
        // rises from 2 at y = 0 to 3 at y = 1. Bottom 1, top sqrt 2, walls 2 and 3 high at
        // y = 0 and y = 1, and two trapezoids from 2 to 3
        let expected = 1.0 + std::f64::consts::SQRT_2 + 2.0 + 3.0 + 2.0 * 2.5;
        assert!((total - expected).abs() < 1e-6, "{total} against {expected}");
        // the top's footprint is the square's
        for v in &mesh.positions {
            assert!(v[0] > -1e-6 && v[0] < 1.0 + 1e-6 && v[1] > -1e-6 && v[1] < 1.0 + 1e-6);
        }
    }

    #[test]
    fn a_cylinder_of_revolution_bends_into_parts() {
        let line = Curve::Line { from: DVec3::new(1.0, 0.0, 0.0), to: DVec3::new(1.0, 0.0, 2.0), t: [0.0, 2.0] };
        let tau = std::f64::consts::TAU;
        let s = Surface::Revolution { from: DVec3::ZERO, axis: DVec3::Z, angle: [0.0, tau], t: [0.0, tau], transposed: false, curve: line };
        let mesh = face_mesh(&s, Vec::new(), false).unwrap();
        for (p, n) in mesh.positions.iter().zip(mesh.normals.as_ref().unwrap()) {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            assert!((r - 1.0).abs() < 1e-5);
            // outward normals
            assert!(n[0] * p[0] + n[1] * p[1] > 0.9);
        }
        assert!(mesh.triangles >= 2 * 30, "{}", mesh.triangles);
    }
}
