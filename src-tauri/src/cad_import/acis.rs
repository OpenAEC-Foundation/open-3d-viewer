//! Tessellation of ACIS B-rep bodies (DWG 3DSOLID, BODY, REGION and SURFACE entities).
//!
//! acadrust decodes the SAT/SAB records; this module turns each face into triangles:
//! the face's loops are sampled (straight, elliptical and B-spline edges), projected into
//! the parameter space of the face surface (plane, cylinder/cone, sphere, torus or B-spline),
//! triangulated there with ear clipping and mapped back onto the surface, with extra
//! subdivision on curved surfaces. Faces that cannot be triangulated are returned as
//! their boundary edges (line segments) so the shape stays visible.

use acadrust::entities::acis::{
    SabReader, SatBSplineSurface, SatCoedge, SatConeSurface, SatDocument, SatEdge, SatEllipseCurve,
    SatFace, SatIntCurve, SatLoop, SatPlaneSurface, SatPoint, SatPointer, SatSphereSurface,
    SatSplineSurface, SatTorusSurface, SatTransform, SatVertex, Sense,
};
use acadrust::entities::AcisData;
use std::collections::{HashMap, HashSet};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

pub type V = [f64; 3];

fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V, s: f64) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: V) -> f64 {
    dot(a, a).sqrt()
}
fn norm(a: V) -> V {
    let l = len(a);
    if l > 1e-300 {
        mul(a, 1.0 / l)
    } else {
        [0.0; 3]
    }
}
fn tup(t: (f64, f64, f64)) -> V {
    [t.0, t.1, t.2]
}
/// Any unit vector perpendicular to `n`.
fn perpendicular(n: V) -> V {
    let a = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    norm(cross(n, a))
}

/// Triangles (with per-vertex normals) plus fallback edge lines, in ACIS model coordinates.
#[derive(Default)]
pub struct Tess {
    pub positions: Vec<V>,
    pub normals: Vec<V>,
    pub indices: Vec<u32>,
    /// Line segments (pairs of points) for faces that could not be triangulated.
    pub lines: Vec<V>,
    pub faces_ok: usize,
    pub faces_failed: usize,
}

/// Parsed ACIS data of an entity (text SAT or binary SAB).
pub fn sat_document(acis: &AcisData) -> Option<SatDocument> {
    if acis.is_binary {
        if acis.sab_data.is_empty() {
            return None;
        }
        SabReader::read(&acis.sab_data).ok()
    } else {
        if acis.sat_data.trim().is_empty() {
            return None;
        }
        SatDocument::parse(&acis.sat_data).ok()
    }
}

/// Tessellate every face of the document into `out`.
pub fn tessellate(doc: &SatDocument, out: &mut Tess) {
    let first_vertex = out.positions.len();
    let first_line = out.lines.len();
    for face in doc.faces() {
        let before = (out.positions.len(), out.indices.len());
        match tessellate_face(doc, &face, out) {
            Ok(()) => out.faces_ok += 1,
            Err(()) => {
                out.positions.truncate(before.0);
                out.normals.truncate(before.0);
                out.indices.truncate(before.1);
                out.faces_failed += 1;
                face_edges(doc, &face, &mut out.lines);
            }
        }
    }
    // A body transform (rare in DWG) maps the whole body.
    if let Some(xf) = body_transform(doc) {
        for p in &mut out.positions[first_vertex..] {
            *p = xf.point(*p);
        }
        for n in &mut out.normals[first_vertex..] {
            *n = norm(xf.vector(*n));
        }
        for p in &mut out.lines[first_line..] {
            *p = xf.point(*p);
        }
    }
}

struct Affine {
    rows: [V; 3],
    t: V,
    scale: f64,
}

impl Affine {
    // ACIS uses row vectors: p' = (p * R) * scale + t
    fn vector(&self, p: V) -> V {
        let r = &self.rows;
        mul(
            [
                p[0] * r[0][0] + p[1] * r[1][0] + p[2] * r[2][0],
                p[0] * r[0][1] + p[1] * r[1][1] + p[2] * r[2][1],
                p[0] * r[0][2] + p[1] * r[1][2] + p[2] * r[2][2],
            ],
            self.scale,
        )
    }
    fn point(&self, p: V) -> V {
        add(self.vector(p), self.t)
    }
}

fn body_transform(doc: &SatDocument) -> Option<Affine> {
    let body = doc.bodies().into_iter().next()?;
    let ptr = body.transform();
    if ptr.is_null() {
        return None;
    }
    let t = SatTransform::from_record(doc.resolve(ptr)?)?;
    let a = Affine { rows: [tup(t.row0()), tup(t.row1()), tup(t.row2())], t: tup(t.translation()), scale: t.scale() };
    let identity = a.rows == [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] && a.t == [0.0; 3] && a.scale == 1.0;
    (!identity && a.scale.is_finite() && a.scale != 0.0).then_some(a)
}

// ---------------------------------------------------------------------------------------------
// Edges
// ---------------------------------------------------------------------------------------------

fn vertex_point(doc: &SatDocument, ptr: SatPointer) -> Option<V> {
    let v = SatVertex::from_record(doc.resolve(ptr)?)?;
    let p = tup(SatPoint::from_record(doc.resolve(v.point())?)?.position());
    p.iter().all(|c| c.is_finite()).then_some(p)
}

/// Points along a coedge in loop direction (first and last point included).
fn sample_coedge(doc: &SatDocument, coedge: &SatCoedge) -> Option<Vec<V>> {
    let edge = SatEdge::from_record(doc.resolve(coedge.edge())?)?;
    let start = vertex_point(doc, edge.start_vertex())?;
    let end = vertex_point(doc, edge.end_vertex())?;
    let reversed_edge = edge.sense() == Sense::Reversed;
    let mut pts = match doc.resolve(edge.curve()) {
        Some(curve) => {
            if let Some(e) = SatEllipseCurve::from_record(curve) {
                let span = (edge.end_param() - edge.start_param()).abs();
                sample_ellipse(&e, start, end, reversed_edge, span.is_finite().then_some(span))
            } else if let Some(c) = SatIntCurve::from_record(curve) {
                sample_intcurve(doc, &c, start, end, reversed_edge).unwrap_or_else(|| vec![start, end])
            } else {
                vec![start, end]
            }
        }
        None => vec![start, end],
    };
    if coedge.sense() == Sense::Reversed {
        pts.reverse();
    }
    Some(pts)
}

const ARC_STEP: f64 = PI / 18.0; // 10 degrees

fn sample_ellipse(e: &SatEllipseCurve, start: V, end: V, reversed: bool, param_span: Option<f64>) -> Vec<V> {
    let c = tup(e.center());
    let n = norm(tup(e.normal()));
    let m = tup(e.major_axis());
    let r = len(m);
    let ratio = e.ratio();
    if r < 1e-12 || len(n) < 0.5 {
        return vec![start, end];
    }
    let mn = cross(n, m); // same length as m, perpendicular
    let minor = mul(mn, ratio);
    let param = |p: V| {
        let d = sub(p, c);
        let x = dot(d, m) / (r * r);
        let y = dot(d, mn) / (r * r) / ratio.max(1e-12);
        y.atan2(x)
    };
    let t0 = param(start);
    let t1 = param(end);
    let closed = len(sub(start, end)) <= 1e-9 * r.max(1.0);
    let at = |t: f64| add(c, add(mul(m, t.cos()), mul(minor, t.sin())));
    let mut sweep = if reversed { -(t0 - t1).rem_euclid(TAU) } else { (t1 - t0).rem_euclid(TAU) };
    if closed || sweep.abs() < 1e-9 {
        sweep = if reversed { -TAU } else { TAU };
    } else if let Some(span) = param_span {
        // The edge's parameter range gives the arc length in radians; pick the turning
        // direction that reaches the end vertex with it (the sense alone is not reliable).
        if span > 1e-9 && span < TAU - 1e-6 {
            let d_pos = len(sub(at(t0 + span), end));
            let d_neg = len(sub(at(t0 - span), end));
            let tol = 1e-6 * r.max(1e-9);
            // only when the parameters are angles, i.e. one direction lands on the end vertex
            if d_pos.min(d_neg) < 1e-4 * r && (d_pos - d_neg).abs() > tol {
                sweep = if d_pos < d_neg { span } else { -span };
            }
        }
    }
    let n_seg = ((sweep.abs() / ARC_STEP).ceil() as usize).max(1);
    let mut pts = Vec::with_capacity(n_seg + 1);
    pts.push(start);
    for i in 1..n_seg {
        let t = t0 + sweep * i as f64 / n_seg as f64;
        pts.push(add(c, add(mul(m, t.cos()), mul(minor, t.sin()))));
    }
    pts.push(end);
    pts
}

/// Homogeneous de Boor evaluation of a B-spline curve.
fn eval_curve(degree: usize, knots: &[f64], ctrl: &[[f64; 4]], t: f64) -> V {
    let n = ctrl.len();
    let span = find_span(n, degree, knots, t);
    let basis = basis_functions(span, t, degree, knots);
    let mut acc = [0.0; 4];
    for (j, b) in basis.iter().enumerate() {
        let p = ctrl[(span - degree + j).min(n - 1)];
        for k in 0..4 {
            acc[k] += b * p[k];
        }
    }
    let w = if acc[3].abs() > 1e-300 { acc[3] } else { 1.0 };
    [acc[0] / w, acc[1] / w, acc[2] / w]
}

fn find_span(n: usize, degree: usize, knots: &[f64], t: f64) -> usize {
    // valid spans: degree ..= n-1 (knots.len() >= n + degree + 1)
    let hi = n - 1;
    if t >= knots[n] {
        return hi;
    }
    let mut span = degree;
    while span < hi && knots[span + 1] <= t {
        span += 1;
    }
    span
}

fn basis_functions(span: usize, t: f64, degree: usize, knots: &[f64]) -> Vec<f64> {
    let mut n = vec![0.0; degree + 1];
    let mut left = vec![0.0; degree + 1];
    let mut right = vec![0.0; degree + 1];
    n[0] = 1.0;
    for j in 1..=degree {
        left[j] = t - knots.get(span + 1 - j).copied().unwrap_or(0.0);
        right[j] = knots.get(span + j).copied().unwrap_or(0.0) - t;
        let mut saved = 0.0;
        for r in 0..j {
            let denom = right[r + 1] + left[j - r];
            let temp = if denom.abs() > 1e-300 { n[r] / denom } else { 0.0 };
            n[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        n[j] = saved;
    }
    n
}

fn sample_intcurve(doc: &SatDocument, c: &SatIntCurve, start: V, end: V, reversed: bool) -> Option<Vec<V>> {
    let (degree, knots, ctrl) = c.bspline_in(doc)?;
    if ctrl.len() <= degree || knots.len() < ctrl.len() + degree + 1 {
        return None;
    }
    let (lo, hi) = (knots[degree], knots[ctrl.len()]);
    if !(hi > lo) {
        return None;
    }
    const N: usize = 96;
    let samples: Vec<V> = (0..=N).map(|i| eval_curve(degree, &knots, &ctrl, lo + (hi - lo) * i as f64 / N as f64)).collect();
    let nearest = |p: V| {
        (0..samples.len())
            .min_by(|&a, &b| len(sub(samples[a], p)).partial_cmp(&len(sub(samples[b], p))).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0)
    };
    let i0 = nearest(start);
    let i1 = nearest(end);
    let curve_closed = len(sub(samples[0], samples[N])) < 1e-6 * (1.0 + len(samples[0]));
    let mut pts = vec![start];
    let forward = !reversed;
    if curve_closed {
        // walk around the closed curve in the edge direction (samples 0 and N coincide)
        let (i0, i1) = (i0 % N, i1 % N);
        let full = len(sub(start, end)) < 1e-9 * (1.0 + len(start));
        let steps = if full {
            N - 1
        } else if forward {
            ((i1 + N - i0) % N).saturating_sub(1)
        } else {
            ((i0 + N - i1) % N).saturating_sub(1)
        };
        for k in 1..=steps {
            let i = if forward { (i0 + k) % N } else { (i0 + N - k % N) % N };
            pts.push(samples[i]);
        }
    } else if i0 <= i1 {
        pts.extend_from_slice(&samples[(i0 + 1).min(i1)..i1]);
    } else {
        pts.extend(samples[(i1 + 1)..i0].iter().rev().copied());
    }
    pts.push(end);
    Some(simplify(&pts, 0.02))
}

/// Douglas-Peucker with a tolerance relative to the chord: keeps a point when the polyline
/// strays from the chord by more than `rel` of the chord length.
fn simplify(pts: &[V], rel: f64) -> Vec<V> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((i, j)) = stack.pop() {
        if j <= i + 1 {
            continue;
        }
        let (a, b) = (pts[i], pts[j]);
        let ab = sub(b, a);
        let l2 = dot(ab, ab);
        let mut worst = (0.0, i);
        for (k, p) in pts.iter().enumerate().take(j).skip(i + 1) {
            let ap = sub(*p, a);
            let d = if l2 > 1e-300 { len(sub(ap, mul(ab, (dot(ap, ab) / l2).clamp(0.0, 1.0)))) } else { len(ap) };
            if d > worst.0 {
                worst = (d, k);
            }
        }
        if worst.0 > rel * l2.sqrt() || (l2 <= 1e-300 && worst.0 > 0.0) {
            keep[worst.1] = true;
            stack.push((i, worst.1));
            stack.push((worst.1, j));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// Each loop of a face as a closed point list (last point not repeated).
fn face_loops(doc: &SatDocument, face: &SatFace) -> Option<Vec<Vec<V>>> {
    let mut loops = Vec::new();
    let mut loop_ptr = face.first_loop();
    let mut seen_loops = HashSet::new();
    while !loop_ptr.is_null() && seen_loops.insert(loop_ptr) {
        let lp = SatLoop::from_record(doc.resolve(loop_ptr)?)?;
        let mut pts: Vec<V> = Vec::new();
        let mut ptr = lp.first_coedge();
        let mut seen = HashSet::new();
        while !ptr.is_null() && seen.insert(ptr) {
            let coedge = SatCoedge::from_record(doc.resolve(ptr)?)?;
            let seg = sample_coedge(doc, &coedge)?;
            // drop the last point: the next coedge starts there
            pts.extend_from_slice(&seg[..seg.len() - 1]);
            ptr = coedge.next();
        }
        // drop consecutive duplicates
        let mut clean: Vec<V> = Vec::with_capacity(pts.len());
        for p in pts {
            if clean.last().is_none_or(|q| len(sub(p, *q)) > 1e-12) {
                clean.push(p);
            }
        }
        while clean.len() > 1 && len(sub(clean[0], *clean.last().unwrap())) <= 1e-12 {
            clean.pop();
        }
        if !clean.is_empty() {
            loops.push(clean);
        }
        loop_ptr = lp.next_loop();
    }
    Some(loops)
}

/// Boundary edges of a face as line segments (fallback for faces we cannot triangulate).
fn face_edges(doc: &SatDocument, face: &SatFace, lines: &mut Vec<V>) {
    if let Some(loops) = face_loops(doc, face) {
        for lp in loops {
            if lp.len() < 2 {
                continue;
            }
            for i in 0..lp.len() {
                lines.push(lp[i]);
                lines.push(lp[(i + 1) % lp.len()]);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Surfaces
// ---------------------------------------------------------------------------------------------

struct BSurf {
    du: usize,
    dv: usize,
    uk: Vec<f64>,
    vk: Vec<f64>,
    nu: usize,
    nv: usize,
    /// control points, index = i * nv + j (u-major) unless `v_major`
    ctrl: Vec<[f64; 4]>,
    v_major: bool,
    /// parameter period of closed directions (0 = open)
    period: [f64; 2],
}

impl BSurf {
    fn from(s: SatBSplineSurface) -> Option<Self> {
        let (nu, nv) = (s.control_count_u, s.control_count_v);
        if nu <= s.degree_u || nv <= s.degree_v || s.control_points.len() < nu * nv {
            return None;
        }
        if s.u_knots.len() < nu + s.degree_u + 1 || s.v_knots.len() < nv + s.degree_v + 1 {
            return None;
        }
        Some(BSurf { du: s.degree_u, dv: s.degree_v, uk: s.u_knots, vk: s.v_knots, nu, nv, ctrl: s.control_points, v_major: false, period: [0.0; 2] })
    }
    fn domain(&self) -> ([f64; 2], [f64; 2]) {
        ([self.uk[self.du], self.uk[self.nu]], [self.vk[self.dv], self.vk[self.nv]])
    }
    fn eval(&self, u: f64, v: f64) -> V {
        let (ud, vd) = self.domain();
        let u = if self.period[0] > 0.0 { ud[0] + (u - ud[0]).rem_euclid(self.period[0]) } else { u.clamp(ud[0], ud[1]) };
        let v = if self.period[1] > 0.0 { vd[0] + (v - vd[0]).rem_euclid(self.period[1]) } else { v.clamp(vd[0], vd[1]) };
        let su = find_span(self.nu, self.du, &self.uk, u);
        let sv = find_span(self.nv, self.dv, &self.vk, v);
        let bu = basis_functions(su, u, self.du, &self.uk);
        let bv = basis_functions(sv, v, self.dv, &self.vk);
        let mut acc = [0.0; 4];
        for (a, nu) in bu.iter().enumerate() {
            let i = (su - self.du + a).min(self.nu - 1);
            for (b, nv) in bv.iter().enumerate() {
                let j = (sv - self.dv + b).min(self.nv - 1);
                let idx = if self.v_major { j * self.nu + i } else { i * self.nv + j };
                let p = self.ctrl[idx];
                let w = nu * nv;
                for k in 0..4 {
                    acc[k] += w * p[k];
                }
            }
        }
        let w = if acc[3].abs() > 1e-300 { acc[3] } else { 1.0 };
        [acc[0] / w, acc[1] / w, acc[2] / w]
    }
}

enum Surf {
    Plane { o: V, ex: V, ey: V },
    /// Elliptical cylinder or cone: c + a h + (1 + k h) (rx cos u mx + ry sin u my)
    Cone { c: V, a: V, mx: V, my: V, rx: f64, ry: f64, k: f64 },
    Sphere { c: V, r: f64, ex: V, ey: V, ez: V },
    Torus { c: V, ex: V, ey: V, ez: V, big: f64, small: f64 },
    Spline { s: BSurf, grid: Vec<(f64, f64, V)> },
}

struct SurfInfo {
    surf: Surf,
    /// +1 when the face normal equals the natural normal (dS/du x dS/dv), else -1
    orient: f64,
}

impl Surf {
    fn eval(&self, u: f64, v: f64) -> V {
        match self {
            Surf::Plane { o, ex, ey } => add(*o, add(mul(*ex, u), mul(*ey, v))),
            Surf::Cone { c, a, mx, my, rx, ry, k } => {
                let s = 1.0 + k * v;
                add(add(*c, mul(*a, v)), mul(add(mul(*mx, rx * u.cos()), mul(*my, ry * u.sin())), s))
            }
            Surf::Sphere { c, r, ex, ey, ez } => {
                let (cv, sv) = (v.cos(), v.sin());
                add(*c, mul(add(add(mul(*ex, cv * u.cos()), mul(*ey, cv * u.sin())), mul(*ez, sv)), *r))
            }
            Surf::Torus { c, ex, ey, ez, big, small } => {
                let rr = big + small * v.cos();
                add(*c, add(mul(add(mul(*ex, u.cos()), mul(*ey, u.sin())), rr), mul(*ez, small * v.sin())))
            }
            Surf::Spline { s, .. } => s.eval(u, v),
        }
    }

    /// Parameters of the surface point closest to `p` (u in (-pi, pi] for periodic angles);
    /// the flag is true at a singular point (pole, apex) where u is undefined.
    fn project(&self, p: V) -> ([f64; 2], bool) {
        match self {
            Surf::Plane { o, ex, ey } => {
                let d = sub(p, *o);
                ([dot(d, *ex), dot(d, *ey)], false)
            }
            Surf::Cone { c, a, mx, my, rx, ry, k } => {
                let d = sub(p, *c);
                let h = dot(d, *a);
                let r = sub(d, mul(*a, h));
                let s = 1.0 + k * h;
                let x = dot(r, *mx) / rx;
                let y = dot(r, *my) / ry;
                let singular = s.abs() < 1e-9 || (x.abs() + y.abs()) < 1e-12 * (1.0 + rx);
                ([y.atan2(x), h], singular)
            }
            Surf::Sphere { c, r, ex, ey, ez } => {
                let d = sub(p, *c);
                let z = dot(d, *ez);
                let x = dot(d, *ex);
                let y = dot(d, *ey);
                let rho = (x * x + y * y).sqrt();
                ([y.atan2(x), z.atan2(rho)], rho < 1e-9 * r.max(1.0))
            }
            Surf::Torus { c, ex, ey, ez, big, small: _ } => {
                let d = sub(p, *c);
                let z = dot(d, *ez);
                let x = dot(d, *ex);
                let y = dot(d, *ey);
                let rho = (x * x + y * y).sqrt();
                ([y.atan2(x), z.atan2(rho - big)], rho < 1e-12)
            }
            Surf::Spline { s, grid } => {
                // nearest grid sample, then Gauss-Newton
                let mut best = grid[0];
                let mut bd = f64::INFINITY;
                for g in grid {
                    let d = len(sub(g.2, p));
                    if d < bd {
                        bd = d;
                        best = *g;
                    }
                }
                let (ud, vd) = s.domain();
                let (mut u, mut v) = (best.0, best.1);
                let hu = (ud[1] - ud[0]) * 1e-5;
                let hv = (vd[1] - vd[0]) * 1e-5;
                for _ in 0..12 {
                    let q = s.eval(u, v);
                    let su = mul(sub(s.eval(u + hu, v), s.eval(u - hu, v)), 0.5 / hu);
                    let sv = mul(sub(s.eval(u, v + hv), s.eval(u, v - hv)), 0.5 / hv);
                    let r = sub(p, q);
                    let (a, b, cc) = (dot(su, su), dot(su, sv), dot(sv, sv));
                    let (e, f) = (dot(su, r), dot(sv, r));
                    let det = a * cc - b * b;
                    if det.abs() < 1e-300 {
                        break;
                    }
                    let du = (e * cc - f * b) / det;
                    let dv = (a * f - b * e) / det;
                    u = if s.period[0] > 0.0 { u + du } else { (u + du).clamp(ud[0], ud[1]) };
                    v = if s.period[1] > 0.0 { v + dv } else { (v + dv).clamp(vd[0], vd[1]) };
                    if du.abs() < hu * 1e-3 && dv.abs() < hv * 1e-3 {
                        break;
                    }
                }
                ([u, v], false)
            }
        }
    }

    /// Period of u and v (0 = not periodic).
    fn periods(&self) -> [f64; 2] {
        match self {
            Surf::Plane { .. } => [0.0, 0.0],
            Surf::Cone { .. } | Surf::Sphere { .. } => [TAU, 0.0],
            Surf::Torus { .. } => [TAU, TAU],
            Surf::Spline { s, .. } => s.period,
        }
    }

    /// Scale of each parameter in model units (makes the ear clipping work on near-isotropic coordinates).
    fn metric(&self) -> [f64; 2] {
        match self {
            Surf::Plane { .. } => [1.0, 1.0],
            Surf::Cone { rx, ry, .. } => [(rx + ry) / 2.0, 1.0],
            Surf::Sphere { r, .. } => [*r, *r],
            Surf::Torus { big, small, .. } => [big + small, *small],
            Surf::Spline { s, .. } => {
                let (ud, vd) = s.domain();
                let mid = [(ud[0] + ud[1]) / 2.0, (vd[0] + vd[1]) / 2.0];
                // arc lengths through the middle (closed directions have coinciding ends)
                let arc = |f: &dyn Fn(f64) -> V| (0..16).map(|i| len(sub(f((i + 1) as f64 / 16.0), f(i as f64 / 16.0)))).sum::<f64>().max(1e-9);
                let lu = arc(&|t| s.eval(ud[0] + (ud[1] - ud[0]) * t, mid[1]));
                let lv = arc(&|t| s.eval(mid[0], vd[0] + (vd[1] - vd[0]) * t));
                [lu / (ud[1] - ud[0]).max(1e-12), lv / (vd[1] - vd[0]).max(1e-12)]
            }
        }
    }

    /// Value of v at a singular boundary (pole/apex) on the given side (+1/-1), if any.
    fn pole(&self, side: f64) -> Option<f64> {
        match self {
            Surf::Sphere { .. } => Some(side * FRAC_PI_2),
            Surf::Cone { k, .. } if k.abs() > 1e-12 => {
                let h0 = -1.0 / k;
                (h0.signum() == side.signum()).then_some(h0)
            }
            _ => None,
        }
    }

    /// The whole surface as a parameter rectangle (closed surfaces only).
    fn full_domain(&self) -> Option<([f64; 2], [f64; 2])> {
        match self {
            Surf::Sphere { .. } => Some(([-PI, PI], [-FRAC_PI_2, FRAC_PI_2])),
            Surf::Torus { .. } => Some(([-PI, PI], [-PI, PI])),
            _ => None,
        }
    }

    /// Characteristic size of the curved surface (radius of curvature, or the extent of
    /// the face points for B-splines).
    fn size(&self, pts: &[V]) -> f64 {
        match self {
            Surf::Plane { .. } => 0.0,
            Surf::Cone { rx, ry, .. } => rx.max(*ry),
            Surf::Sphere { r, .. } => *r,
            Surf::Torus { small, .. } => *small,
            Surf::Spline { .. } => {
                let mut mn = [f64::INFINITY; 3];
                let mut mx = [f64::NEG_INFINITY; 3];
                for p in pts {
                    for k in 0..3 {
                        mn[k] = mn[k].min(p[k]);
                        mx[k] = mx[k].max(p[k]);
                    }
                }
                if mn[0] <= mx[0] {
                    len(sub(mx, mn)) / 2.0
                } else {
                    0.0
                }
            }
        }
    }

    /// Natural normal (dS/du x dS/dv) by central differences.
    fn normal(&self, u: f64, v: f64) -> V {
        let (hu, hv) = match self {
            Surf::Plane { ex, ey, .. } => return norm(cross(*ex, *ey)),
            Surf::Spline { s, .. } => {
                let (ud, vd) = s.domain();
                ((ud[1] - ud[0]) * 1e-5, (vd[1] - vd[0]) * 1e-5)
            }
            _ => (1e-6, 1e-6),
        };
        let su = sub(self.eval(u + hu, v), self.eval(u - hu, v));
        let sv = sub(self.eval(u, v + hv), self.eval(u, v - hv));
        norm(cross(su, sv))
    }
}

fn surface_info(doc: &SatDocument, face: &SatFace, loops: &[Vec<V>]) -> Option<SurfInfo> {
    let rec = doc.resolve(face.surface())?;
    let face_sign = if face.sense() == Sense::Reversed { -1.0 } else { 1.0 };
    let sign = |s: Sense| if s == Sense::Reversed { -1.0 } else { 1.0 };
    if let Some(p) = SatPlaneSurface::from_record(rec) {
        let n = norm(tup(p.normal()));
        if len(n) < 0.5 {
            return None;
        }
        let mut ex = tup(p.u_direction());
        ex = sub(ex, mul(n, dot(ex, n)));
        let ex = if len(ex) > 1e-9 { norm(ex) } else { perpendicular(n) };
        let ey = cross(n, ex);
        return Some(SurfInfo { surf: Surf::Plane { o: tup(p.root_point()), ex, ey }, orient: face_sign * sign(p.sense()) });
    }
    if let Some(c) = SatConeSurface::from_record(rec) {
        let a = norm(tup(c.axis()));
        let m = tup(c.major_axis());
        let rx = len(m);
        let ratio = c.ratio().abs();
        if len(a) < 0.5 || rx < 1e-12 || ratio < 1e-12 {
            return None;
        }
        let mx = norm(m);
        let my = cross(a, mx);
        let ry = rx * ratio;
        let (sin, cos) = (c.sin_half_angle(), c.cos_half_angle());
        let tan = if cos.abs() > 1e-12 { sin / cos } else { 0.0 };
        let center = tup(c.center());
        // The sign convention of the half angle is ambiguous; keep the one that fits the edges.
        let mut k = tan / rx;
        if tan.abs() > 1e-12 {
            let resid = |k: f64| -> f64 {
                loops
                    .iter()
                    .flatten()
                    .map(|p| {
                        let d = sub(*p, center);
                        let h = dot(d, a);
                        let r = sub(d, mul(a, h));
                        let x = dot(r, mx) / rx;
                        let y = dot(r, my) / ry;
                        ((x * x + y * y).sqrt() - (1.0 + k * h)).abs()
                    })
                    .sum()
            };
            if resid(-k) < resid(k) {
                k = -k;
            }
        }
        // a negative cosine of the half angle means the normal points towards the axis
        let inward = if cos < 0.0 { -1.0 } else { 1.0 };
        return Some(SurfInfo { surf: Surf::Cone { c: center, a, mx, my, rx, ry, k }, orient: face_sign * sign(c.sense()) * inward });
    }
    if let Some(s) = SatSphereSurface::from_record(rec) {
        let r = s.radius();
        let ez = norm(tup(s.pole()));
        let mut ex = tup(s.u_direction());
        ex = sub(ex, mul(ez, dot(ex, ez)));
        if len(ez) < 0.5 || r.abs() < 1e-12 {
            return None;
        }
        let ex = if len(ex) > 1e-9 { norm(ex) } else { perpendicular(ez) };
        let ey = cross(ez, ex);
        let orient = face_sign * sign(s.sense()) * r.signum();
        return Some(SurfInfo { surf: Surf::Sphere { c: tup(s.center()), r: r.abs(), ex, ey, ez }, orient });
    }
    if let Some(t) = SatTorusSurface::from_record(rec) {
        let ez = norm(tup(t.normal()));
        let mut ex = tup(t.u_direction());
        ex = sub(ex, mul(ez, dot(ex, ez)));
        let (big, small) = (t.major_radius(), t.minor_radius());
        if len(ez) < 0.5 || small.abs() < 1e-12 {
            return None;
        }
        let ex = if len(ex) > 1e-9 { norm(ex) } else { perpendicular(ez) };
        let ey = cross(ez, ex);
        let orient = face_sign * sign(t.sense()) * small.signum();
        return Some(SurfInfo { surf: Surf::Torus { c: tup(t.center()), ex, ey, ez, big, small: small.abs() }, orient });
    }
    if let Some(s) = SatSplineSurface::from_record(rec) {
        let mut b = BSurf::from(s.bspline(doc)?)?;
        // The control net order is not documented; keep the one that fits the edges best.
        let fit = |b: &BSurf| -> f64 {
            let surf = Surf::Spline { s: b.clone(), grid: spline_grid(b) };
            loops.iter().flatten().take(64).map(|p| len(sub(surf.eval_projected(*p), *p))).sum()
        };
        let f0 = fit(&b);
        b.v_major = true;
        if fit(&b) >= f0 {
            b.v_major = false;
        }
        // closed directions (surfaces of revolution, pipes) are periodic in parameter space
        let (ud, vd) = b.domain();
        let extent = spline_grid(&b).iter().map(|g| len(g.2)).fold(0.0, f64::max);
        let tol = 1e-6 * (1.0 + extent);
        let closed_u = (0..=6).all(|i| {
            let v = vd[0] + (vd[1] - vd[0]) * i as f64 / 6.0;
            len(sub(b.eval(ud[0], v), b.eval(ud[1], v))) <= tol
        });
        let closed_v = (0..=6).all(|i| {
            let u = ud[0] + (ud[1] - ud[0]) * i as f64 / 6.0;
            len(sub(b.eval(u, vd[0]), b.eval(u, vd[1]))) <= tol
        });
        b.period = [if closed_u { ud[1] - ud[0] } else { 0.0 }, if closed_v { vd[1] - vd[0] } else { 0.0 }];
        let grid = spline_grid(&b);
        return Some(SurfInfo { surf: Surf::Spline { s: b, grid }, orient: face_sign * sign(s.sense()) });
    }
    None
}

impl Clone for BSurf {
    fn clone(&self) -> Self {
        BSurf { du: self.du, dv: self.dv, uk: self.uk.clone(), vk: self.vk.clone(), nu: self.nu, nv: self.nv, ctrl: self.ctrl.clone(), v_major: self.v_major, period: self.period }
    }
}

impl SurfInfo {
    /// The same B-spline surface treated as open in both directions (None when it already is).
    fn without_periods(&self) -> Option<SurfInfo> {
        match &self.surf {
            Surf::Spline { s, grid } if s.period != [0.0; 2] => {
                let mut s = s.clone();
                s.period = [0.0; 2];
                Some(SurfInfo { surf: Surf::Spline { s, grid: grid.clone() }, orient: self.orient })
            }
            _ => None,
        }
    }
}

impl Surf {
    fn eval_projected(&self, p: V) -> V {
        let (uv, _) = self.project(p);
        self.eval(uv[0], uv[1])
    }
}

fn spline_grid(s: &BSurf) -> Vec<(f64, f64, V)> {
    let (ud, vd) = s.domain();
    const G: usize = 16;
    let mut grid = Vec::with_capacity((G + 1) * (G + 1));
    for i in 0..=G {
        for j in 0..=G {
            let u = ud[0] + (ud[1] - ud[0]) * i as f64 / G as f64;
            let v = vd[0] + (vd[1] - vd[0]) * j as f64 / G as f64;
            grid.push((u, v, s.eval(u, v)));
        }
    }
    grid
}

// ---------------------------------------------------------------------------------------------
// Faces
// ---------------------------------------------------------------------------------------------

/// A loop in parameter space; `pts3[i]` is the model point of `uv[i]` (None = synthetic).
struct UvLoop {
    uv: Vec<[f64; 2]>,
    pts3: Vec<Option<V>>,
    /// net number of turns around each periodic parameter
    wraps: [i32; 2],
}

fn wrap_near(x: f64, reference: f64, period: f64) -> f64 {
    if period <= 0.0 {
        return x;
    }
    x - ((x - reference) / period).round() * period
}

fn to_uv_loop(surf: &Surf, pts: &[V]) -> UvLoop {
    let periods = surf.periods();
    let raw: Vec<([f64; 2], bool)> = pts.iter().map(|p| surf.project(*p)).collect();
    let n = pts.len();
    // unwrap periodic parameters along the loop (singular points take no part in u)
    let mut uv: Vec<[f64; 2]> = raw.iter().map(|r| r.0).collect();
    let mut prev: Option<[f64; 2]> = None;
    for i in 0..n {
        let (_, singular) = raw[i];
        if let Some(p) = prev {
            for k in 0..2 {
                if k == 0 && singular {
                    continue;
                }
                uv[i][k] = wrap_near(uv[i][k], p[k], periods[k]);
            }
        }
        if !singular {
            prev = Some(uv[i]);
        } else if let Some(p) = prev {
            prev = Some([p[0], uv[i][1]]);
        } else {
            // v still needs unwrapping on a torus-like surface; keep raw
        }
    }
    // net wraps: sum of increments including the closing segment
    let mut wraps = [0i32; 2];
    for k in 0..2 {
        if periods[k] <= 0.0 {
            continue;
        }
        let idx: Vec<usize> = (0..n).filter(|&i| !(k == 0 && raw[i].1)).collect();
        if idx.len() < 2 {
            continue;
        }
        let first = uv[idx[0]][k];
        let last = uv[*idx.last().unwrap()][k];
        let closing = wrap_near(first, last, periods[k]) - last;
        let total = last - first + closing;
        wraps[k] = (total / periods[k]).round() as i32;
    }
    // singular points get the u of their neighbours (two points: incoming and outgoing)
    let mut out_uv = Vec::with_capacity(n + 4);
    let mut out_p = Vec::with_capacity(n + 4);
    for i in 0..n {
        if raw[i].1 && n > 2 {
            let find = |dir: isize| -> Option<f64> {
                (1..n as isize).map(|s| ((i as isize + dir * s).rem_euclid(n as isize)) as usize).find(|&j| !raw[j].1).map(|j| uv[j][0])
            };
            let (a, b) = (find(-1), find(1));
            if let (Some(a), Some(b)) = (a, b) {
                let b = wrap_near(b, a, periods[0]);
                out_uv.push([a, uv[i][1]]);
                out_p.push(Some(pts[i]));
                if (b - a).abs() > 1e-9 {
                    out_uv.push([b, uv[i][1]]);
                    out_p.push(Some(pts[i]));
                }
                continue;
            }
        }
        out_uv.push(uv[i]);
        out_p.push(Some(pts[i]));
    }
    UvLoop { uv: out_uv, pts3: out_p, wraps }
}

fn signed_area(uv: &[[f64; 2]]) -> f64 {
    let mut a = 0.0;
    for i in 0..uv.len() {
        let p = uv[i];
        let q = uv[(i + 1) % uv.len()];
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}

/// Cut a loop that wraps once around periodic parameter `ax` into an open chain that runs
/// in increasing `ax` from `start` to `start + period`.
fn open_chain(lp: &UvLoop, ax: usize, period: f64, start: f64) -> Option<(Vec<[f64; 2]>, Vec<Option<V>>)> {
    let n = lp.uv.len();
    if n < 2 {
        return None;
    }
    // re-unwrap continuously starting at point 0
    let mut uv = lp.uv.clone();
    for i in 1..n {
        uv[i][ax] = wrap_near(uv[i][ax], uv[i - 1][ax], period);
    }
    let mut pts = lp.pts3.clone();
    // make it ascend
    let closing = wrap_near(uv[0][ax], uv[n - 1][ax], period) - uv[n - 1][ax];
    let total = uv[n - 1][ax] - uv[0][ax] + closing;
    if total < 0.0 {
        uv.reverse();
        pts.reverse();
        for i in 1..n {
            uv[i][ax] = wrap_near(uv[i][ax], uv[i - 1][ax], period);
        }
    }
    // closed list as a cyclic sequence ascending by one period; find where it crosses `start`
    let mut seq: Vec<([f64; 2], Option<V>)> = Vec::with_capacity(2 * n + 1);
    for i in 0..=2 * n {
        let j = i % n;
        let mut p = uv[j];
        p[ax] = uv[j][ax] + period * (i / n) as f64;
        // keep continuity across the wrap of the index
        if i >= n {
            p[ax] = wrap_near(p[ax], seq[i - 1].0[ax], period);
        }
        seq.push((p, pts[j]));
    }
    // shift so seq[0] is within [start - period, start)
    let shift = ((start - seq[0].0[ax]) / period).floor() * period;
    for s in &mut seq {
        s.0[ax] += shift;
    }
    // find first segment crossing `start`
    let mut cut = None;
    for i in 0..2 * n {
        let (a, b) = (seq[i].0[ax], seq[i + 1].0[ax]);
        if a <= start && b > start {
            cut = Some(i);
            break;
        }
    }
    let i = cut?;
    let lerp = |a: [f64; 2], b: [f64; 2], x: f64| {
        let t = if (b[ax] - a[ax]).abs() > 1e-300 { (x - a[ax]) / (b[ax] - a[ax]) } else { 0.0 };
        let mut r = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        r[ax] = x;
        r
    };
    let mut out_uv = vec![lerp(seq[i].0, seq[i + 1].0, start)];
    let mut out_p = vec![None];
    let mut j = i + 1;
    while j < seq.len() && seq[j].0[ax] < start + period {
        out_uv.push(seq[j].0);
        out_p.push(seq[j].1);
        j += 1;
        if j > i + n {
            break;
        }
    }
    let k = (j.min(seq.len() - 1)).max(1);
    out_uv.push(lerp(seq[k - 1].0, seq[k].0, start + period));
    out_p.push(None);
    Some((out_uv, out_p))
}

struct Ring {
    uv: Vec<[f64; 2]>,
    pts3: Vec<Option<V>>,
}

fn build_rings(info: &SurfInfo, loops: &[UvLoop]) -> Option<Vec<(Ring, Vec<Ring>)>> {
    let surf = &info.surf;
    let periods = surf.periods();
    let o = info.orient;
    let wrapping: Vec<(usize, usize)> = loops
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            if l.wraps[0] != 0 && l.wraps[1] != 0 {
                Some((i, 9))
            } else if l.wraps[0] != 0 {
                Some((i, 0))
            } else if l.wraps[1] != 0 {
                Some((i, 1))
            } else {
                None
            }
        })
        .collect();
    if wrapping.iter().any(|w| w.1 == 9 || loops[w.0].wraps[w.1].abs() != 1) {
        return None;
    }
    let mut rings: Vec<Ring> = Vec::new();
    let mut holes: Vec<Ring> = Vec::new();
    let plain: Vec<&UvLoop> = loops.iter().filter(|l| l.wraps == [0, 0]).collect();

    // the face lies on the left of each loop: for a loop running along +u that is +v
    // when the face normal is the natural one
    let side_of = |l: &UvLoop, ax: usize| -> f64 {
        let dir = l.wraps[ax].signum() as f64;
        if ax == 0 {
            dir * o
        } else {
            -dir * o
        }
    };

    match wrapping.len() {
        0 => {
            for l in &plain {
                if l.uv.len() < 3 {
                    continue;
                }
                let a = signed_area(&l.uv);
                let ring = Ring { uv: l.uv.clone(), pts3: l.pts3.clone() };
                if a * o > 0.0 {
                    rings.push(ring);
                } else {
                    holes.push(ring);
                }
            }
            if rings.is_empty() {
                if let Some((_, vd)) = surf.full_domain() {
                    // centre the domain on the holes so they do not straddle the seam
                    let cu = holes.first().map(|h| h.uv.iter().map(|p| p[0]).sum::<f64>() / h.uv.len() as f64).unwrap_or(0.0);
                    let cv = holes.first().map(|h| h.uv.iter().map(|p| p[1]).sum::<f64>() / h.uv.len() as f64).unwrap_or(0.0);
                    let (u0, u1) = (cu - PI, cu + PI);
                    let (v0, v1) = if periods[1] > 0.0 { (cv - PI, cv + PI) } else { (vd[0], vd[1]) };
                    rings.push(rect_ring(u0, u1, v0, v1, 24));
                } else if !holes.is_empty() {
                    // orientation disagrees with what we expect: use the largest loop as the outline
                    holes.sort_by(|a, b| signed_area(&b.uv).abs().partial_cmp(&signed_area(&a.uv).abs()).unwrap_or(std::cmp::Ordering::Equal));
                    rings.push(holes.remove(0));
                } else {
                    return None;
                }
            }
        }
        1 => {
            let (li, ax) = wrapping[0];
            let l = &loops[li];
            let side = side_of(l, ax);
            let start = l.uv.iter().map(|p| p[ax]).fold(f64::INFINITY, f64::min);
            let (mut uv, mut p3) = open_chain(l, ax, periods[ax], start)?;
            let other = 1 - ax;
            // a cone has one apex: use it whichever side the loop orientation suggests
            let cap = if ax == 0 { surf.pole(side).or_else(|| if matches!(surf, Surf::Cone { .. }) { surf.pole(-side) } else { None }) } else { None };
            let cap = match cap {
                Some(c) => c,
                None => {
                    // closed in the other direction too (torus): close over a full period
                    if periods[other] > 0.0 {
                        let mean = uv.iter().map(|p| p[other]).sum::<f64>() / uv.len() as f64;
                        mean + side * periods[other]
                    } else {
                        return None;
                    }
                }
            };
            let end = uv.last()?[ax];
            let begin = uv[0][ax];
            let steps = 24;
            for s in 0..=steps {
                let t = end + (begin - end) * s as f64 / steps as f64;
                let mut q = [0.0; 2];
                q[ax] = t;
                q[other] = cap;
                uv.push(q);
                p3.push(None);
            }
            rings.push(Ring { uv, pts3: p3 });
            for l in &plain {
                holes.push(Ring { uv: l.uv.clone(), pts3: l.pts3.clone() });
            }
        }
        2 => {
            let (ia, ax) = wrapping[0];
            let (ib, bx) = wrapping[1];
            if ax != bx {
                return None;
            }
            let other = 1 - ax;
            let la = &loops[ia];
            let lb = &loops[ib];
            let start = la.uv.iter().map(|p| p[ax]).fold(f64::INFINITY, f64::min);
            let (ua, pa) = open_chain(la, ax, periods[ax], start)?;
            let (mut ub, mut pb) = open_chain(lb, ax, periods[ax], start)?;
            if periods[other] > 0.0 {
                // put B on the face side of A, within one period
                let side = side_of(la, ax);
                let ma = ua.iter().map(|p| p[other]).sum::<f64>() / ua.len() as f64;
                let mb = ub.iter().map(|p| p[other]).sum::<f64>() / ub.len() as f64;
                let mut shift = wrap_near(mb, ma, periods[other]) - mb;
                let rel = mb + shift - ma;
                if rel * side < 0.0 {
                    shift += side * periods[other];
                }
                for p in &mut ub {
                    p[other] += shift;
                }
            }
            ub.reverse();
            pb.reverse();
            let mut uv = ua;
            let mut p3 = pa;
            uv.extend(ub);
            p3.extend(pb);
            rings.push(Ring { uv, pts3: p3 });
            for l in &plain {
                holes.push(Ring { uv: l.uv.clone(), pts3: l.pts3.clone() });
            }
        }
        _ => return None,
    }

    // Holes on periodic surfaces: move them next to the outline.
    if periods[0] > 0.0 || periods[1] > 0.0 {
        let outer = &rings[0];
        let c: [f64; 2] = [
            outer.uv.iter().map(|p| p[0]).sum::<f64>() / outer.uv.len() as f64,
            outer.uv.iter().map(|p| p[1]).sum::<f64>() / outer.uv.len() as f64,
        ];
        for h in &mut holes {
            for k in 0..2 {
                if periods[k] > 0.0 {
                    let m = h.uv.iter().map(|p| p[k]).sum::<f64>() / h.uv.len() as f64;
                    let shift = wrap_near(m, c[k], periods[k]) - m;
                    for p in &mut h.uv {
                        p[k] += shift;
                    }
                }
            }
        }
    }
    // Several outer loops on one face (unusual): the holes go with the first one.
    let mut result: Vec<(Ring, Vec<Ring>)> = Vec::new();
    for r in rings {
        let h = if result.is_empty() { std::mem::take(&mut holes) } else { Vec::new() };
        result.push((r, h));
    }
    Some(result)
}

fn rect_ring(u0: f64, u1: f64, v0: f64, v1: f64, steps: usize) -> Ring {
    let mut uv = Vec::new();
    for s in 0..steps {
        uv.push([u0 + (u1 - u0) * s as f64 / steps as f64, v0]);
    }
    for s in 0..steps {
        uv.push([u1, v0 + (v1 - v0) * s as f64 / steps as f64]);
    }
    for s in 0..steps {
        uv.push([u1 - (u1 - u0) * s as f64 / steps as f64, v1]);
    }
    for s in 0..steps {
        uv.push([u0, v1 - (v1 - v0) * s as f64 / steps as f64]);
    }
    let n = uv.len();
    Ring { uv, pts3: vec![None; n] }
}

/// Maximum sag of a curved surface below a triangle edge, relative to the edge length.
const SAG: f64 = 0.03;
/// Refinement stops beyond this many triangles in one face.
const MAX_FACE_TRIANGLES: usize = 20_000;

fn tessellate_face(doc: &SatDocument, face: &SatFace, out: &mut Tess) -> Result<(), ()> {
    let loops3 = face_loops(doc, face).ok_or(())?;
    let before = (out.positions.len(), out.indices.len());
    let ok = match surface_info(doc, face, &loops3) {
        Some(info) => {
            tessellate_on_surface(&info, &loops3, out)
                || info.without_periods().is_some_and(|plain| {
                    // a B-spline that only looked closed: retry as an open patch
                    out.positions.truncate(before.0);
                    out.normals.truncate(before.0);
                    out.indices.truncate(before.1);
                    tessellate_on_surface(&plain, &loops3, out)
                })
        }
        None => false,
    };
    if ok {
        return Ok(());
    }
    out.positions.truncate(before.0);
    out.normals.truncate(before.0);
    out.indices.truncate(before.1);
    // unsupported surface definition: fill nearly flat faces on their best-fit plane
    tessellate_flat(&loops3, out).then_some(()).ok_or(())
}

fn tessellate_on_surface(info: &SurfInfo, loops3: &[Vec<V>], out: &mut Tess) -> bool {
    let surf = &info.surf;
    if loops3.iter().all(|l| l.is_empty()) {
        // a closed surface without boundary (whole sphere or torus): a regular grid
        let Some((ud, vd)) = surf.full_domain() else {
            return false;
        };
        let (nu, nv) = (32usize, if matches!(surf, Surf::Sphere { .. }) { 16usize } else { 24 });
        let base = out.positions.len() as u32;
        for j in 0..=nv {
            for i in 0..=nu {
                let (u, v) = (ud[0] + (ud[1] - ud[0]) * i as f64 / nu as f64, vd[0] + (vd[1] - vd[0]) * j as f64 / nv as f64);
                out.positions.push(surf.eval(u, v));
                out.normals.push(mul(surf.normal(u, v), info.orient));
            }
        }
        let id = |i: usize, j: usize| base + (j * (nu + 1) + i) as u32;
        for j in 0..nv {
            for i in 0..nu {
                emit_triangle(out, id(i, j), id(i + 1, j), id(i + 1, j + 1));
                emit_triangle(out, id(i, j), id(i + 1, j + 1), id(i, j + 1));
            }
        }
        return true;
    }
    let uv_loops: Vec<UvLoop> = loops3.iter().filter(|l| !l.is_empty()).map(|l| to_uv_loop(surf, l)).collect();
    let Some(groups) = build_rings(info, &uv_loops) else {
        return false;
    };
    let before = out.indices.len();
    for (outer, holes) in &groups {
        if tessellate_rings(info, outer, holes, out).is_err() {
            return false;
        }
    }
    out.indices.len() > before
}

/// Fallback for faces on surfaces we cannot evaluate: ear clipping on the best-fit plane of
/// the loops, only when the loops are close to that plane.
fn tessellate_flat(loops3: &[Vec<V>], out: &mut Tess) -> bool {
    let mut loops: Vec<&Vec<V>> = loops3.iter().filter(|l| l.len() >= 3).collect();
    if loops.is_empty() {
        return false;
    }
    let newell = |l: &[V]| {
        let mut n = [0.0; 3];
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            n[0] += (a[1] - b[1]) * (a[2] + b[2]);
            n[1] += (a[2] - b[2]) * (a[0] + b[0]);
            n[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        n
    };
    loops.sort_by(|a, b| len(newell(b)).partial_cmp(&len(newell(a))).unwrap_or(std::cmp::Ordering::Equal));
    let n = norm(newell(loops[0]));
    if len(n) < 0.5 {
        return false;
    }
    let pts: Vec<V> = loops.iter().flat_map(|l| l.iter().copied()).collect();
    let c = mul(pts.iter().fold([0.0; 3], |s, p| add(s, *p)), 1.0 / pts.len() as f64);
    let mut mn = [f64::INFINITY; 3];
    let mut mx = [f64::NEG_INFINITY; 3];
    for p in &pts {
        for k in 0..3 {
            mn[k] = mn[k].min(p[k]);
            mx[k] = mx[k].max(p[k]);
        }
    }
    let size = len(sub(mx, mn));
    if pts.iter().any(|p| dot(sub(*p, c), n).abs() > 0.03 * size) {
        return false;
    }
    let ex = perpendicular(n);
    let ey = cross(n, ex);
    let mut coords = Vec::new();
    let mut holes = Vec::new();
    for (i, l) in loops.iter().enumerate() {
        if i > 0 {
            holes.push(pts_len(&coords));
        }
        for p in l.iter() {
            let d = sub(*p, c);
            coords.push(dot(d, ex));
            coords.push(dot(d, ey));
        }
    }
    let Ok(mut tris) = earcutr::earcut(&coords, &holes, 2) else {
        return false;
    };
    if tris.is_empty() {
        return false;
    }
    delaunay_flips(&coords, &mut tris);
    let base = out.positions.len() as u32;
    for p in &pts {
        out.positions.push(*p);
        out.normals.push(n);
    }
    for t in tris.chunks_exact(3) {
        emit_triangle(out, base + t[0] as u32, base + t[1] as u32, base + t[2] as u32);
    }
    true
}

fn pts_len(coords: &[f64]) -> usize {
    coords.len() / 2
}

/// Ear-clip one outline with its holes in parameter space and map the triangles onto the surface.
fn tessellate_rings(info: &SurfInfo, outer: &Ring, holes: &[Ring], out: &mut Tess) -> Result<(), ()> {
    let surf = &info.surf;
    let metric = surf.metric();
    let mut coords: Vec<f64> = Vec::new();
    let mut hole_idx: Vec<usize> = Vec::new();
    let mut verts_uv: Vec<[f64; 2]> = Vec::new();
    let mut verts_p: Vec<Option<V>> = Vec::new();
    if outer.uv.len() < 3 {
        return Err(());
    }
    for (ri, r) in std::iter::once(outer).chain(holes.iter()).enumerate() {
        if r.uv.len() < 3 {
            continue;
        }
        if ri > 0 {
            hole_idx.push(verts_uv.len());
        }
        for (k, p) in r.uv.iter().enumerate() {
            coords.push(p[0] * metric[0]);
            coords.push(p[1] * metric[1]);
            verts_uv.push(*p);
            verts_p.push(r.pts3[k]);
        }
    }
    if coords.iter().any(|c| !c.is_finite()) {
        return Err(());
    }
    let mut tris = earcutr::earcut(&coords, &hole_idx, 2).map_err(|_| ())?;
    if tris.is_empty() {
        return Err(());
    }
    delaunay_flips(&coords, &mut tris);

    // model position of every vertex: exact for sampled edge points, else on the surface
    let mut pos: Vec<V> = (0..verts_uv.len()).map(|i| verts_p[i].unwrap_or_else(|| surf.eval(verts_uv[i][0], verts_uv[i][1]))).collect();
    // the refinement test uses surface points only: sampled edge points may lie slightly
    // off an approximated (B-spline) surface and must not trigger endless splitting
    let mut on_surf: Vec<V> = (0..verts_uv.len()).map(|i| if verts_p[i].is_some() { surf.eval(verts_uv[i][0], verts_uv[i][1]) } else { pos[i] }).collect();

    // Refine curved surfaces: every edge whose midpoint on the surface sags away from the
    // edge by more than SAG of its length (about 14 degrees of arc) is halved, then the
    // triangulation is made Delaunay again, until all edges fit. Edges are decided once per
    // round, so neighbouring triangles always split a shared edge alike (no cracks).
    if !matches!(surf, Surf::Plane { .. }) {
        // ignore deviations far below the size of the surface (small circles near poles)
        let abs_tol = 0.004 * surf.size(&pos);
        let needs_split = |a: [f64; 2], b: [f64; 2], pa: V, pb: V| -> Option<V> {
            let chord = len(sub(pa, pb));
            let m = surf.eval((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
            let sag = len(sub(m, mul(add(pa, pb), 0.5)));
            // an edge that is degenerate in 3D (seam) can still span the whole surface
            (sag > SAG * chord && sag > abs_tol).then_some(m)
        };
        let key = |a: usize, b: usize| if a < b { (a, b) } else { (b, a) };
        for _ in 0..16 {
            let mut decided: HashMap<(usize, usize), Option<usize>> = HashMap::new();
            for t in tris.chunks_exact(3) {
                for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                    let k = key(a, b);
                    if decided.contains_key(&k) {
                        continue;
                    }
                    let (ua, ub) = (verts_uv[k.0], verts_uv[k.1]);
                    let split = needs_split(ua, ub, on_surf[k.0], on_surf[k.1]).map(|m| {
                        let uv = [(ua[0] + ub[0]) / 2.0, (ua[1] + ub[1]) / 2.0];
                        verts_uv.push(uv);
                        verts_p.push(None);
                        coords.push(uv[0] * metric[0]);
                        coords.push(uv[1] * metric[1]);
                        pos.push(m);
                        on_surf.push(m);
                        pos.len() - 1
                    });
                    decided.insert(k, split);
                }
            }
            if decided.values().all(|s| s.is_none()) {
                break;
            }
            let mut next = Vec::with_capacity(tris.len() * 2);
            for t in tris.chunks_exact(3) {
                let m = [decided[&key(t[0], t[1])], decided[&key(t[1], t[2])], decided[&key(t[2], t[0])]];
                let count = m.iter().filter(|s| s.is_some()).count();
                // rotate so the split edges come first: edge i = (t[i], t[i+1])
                let r = match count {
                    0 => {
                        next.extend_from_slice(t);
                        continue;
                    }
                    1 => m.iter().position(|s| s.is_some()).unwrap(),
                    2 => m.iter().position(|s| s.is_none()).map(|i| (i + 1) % 3).unwrap(),
                    _ => 0,
                };
                let (a, b, c) = (t[r], t[(r + 1) % 3], t[(r + 2) % 3]);
                let (mab, mbc, mca) = (m[r], m[(r + 1) % 3], m[(r + 2) % 3]);
                match count {
                    1 => {
                        let ab = mab.unwrap();
                        next.extend_from_slice(&[a, ab, c, ab, b, c]);
                    }
                    2 => {
                        let (ab, bc) = (mab.unwrap(), mbc.unwrap());
                        next.extend_from_slice(&[ab, b, bc, a, ab, bc, a, bc, c]);
                    }
                    _ => {
                        let (ab, bc, ca) = (mab.unwrap(), mbc.unwrap(), mca.unwrap());
                        next.extend_from_slice(&[a, ab, ca, ab, b, bc, ca, bc, c, ab, bc, ca]);
                    }
                }
            }
            tris = next;
            delaunay_flips(&coords, &mut tris);
            if tris.len() / 3 > MAX_FACE_TRIANGLES {
                break;
            }
        }
    }

    let o = info.orient;
    let plane_normal = if let Surf::Plane { ex, ey, .. } = surf { Some(mul(norm(cross(*ex, *ey)), o)) } else { None };
    let base = out.positions.len() as u32;
    for (i, p) in pos.iter().enumerate() {
        out.positions.push(*p);
        out.normals.push(plane_normal.unwrap_or_else(|| mul(surf.normal(verts_uv[i][0], verts_uv[i][1]), o)));
    }
    for t in tris.chunks_exact(3) {
        emit_triangle(out, base + t[0] as u32, base + t[1] as u32, base + t[2] as u32);
    }
    Ok(())
}

/// Improve an ear-clipped triangulation towards a constrained Delaunay one (Lawson edge
/// flips). Ear clipping often makes long fans; on curved surfaces those would need heavy
/// refinement. Boundary edges belong to one triangle only and are never flipped.
pub(crate) fn delaunay_flips(coords: &[f64], tris: &mut [usize]) {
    let p = |i: usize| [coords[2 * i], coords[2 * i + 1]];
    let orient = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    // make every triangle counter-clockwise
    for t in tris.chunks_exact_mut(3) {
        if orient(p(t[0]), p(t[1]), p(t[2])) < 0.0 {
            t.swap(1, 2);
        }
    }
    let n_tris = tris.len() / 3;
    let key = |a: usize, b: usize| if a < b { (a, b) } else { (b, a) };
    let mut edges: HashMap<(usize, usize), Vec<usize>> = HashMap::with_capacity(n_tris * 3);
    for t in 0..n_tris {
        for k in 0..3 {
            edges.entry(key(tris[3 * t + k], tris[3 * t + (k + 1) % 3])).or_default().push(t);
        }
    }
    let mut queue: Vec<(usize, usize)> = edges.iter().filter(|(_, v)| v.len() == 2).map(|(k, _)| *k).collect();
    let in_circle = |a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]| {
        let (adx, ady) = (a[0] - d[0], a[1] - d[1]);
        let (bdx, bdy) = (b[0] - d[0], b[1] - d[1]);
        let (cdx, cdy) = (c[0] - d[0], c[1] - d[1]);
        let det = (adx * adx + ady * ady) * (bdx * cdy - cdx * bdy) - (bdx * bdx + bdy * bdy) * (adx * cdy - cdx * ady)
            + (cdx * cdx + cdy * cdy) * (adx * bdy - bdx * ady);
        let scale = (adx * adx + ady * ady + bdx * bdx + bdy * bdy + cdx * cdx + cdy * cdy).powi(2);
        det > 1e-12 * scale
    };
    // fans from ear clipping can need O(n^2) flips
    let mut budget = (n_tris * n_tris * 4).clamp(4_000, 40_000_000);
    while let Some(e) = queue.pop() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        let Some(ts) = edges.get(&e) else { continue };
        if ts.len() != 2 {
            continue;
        }
        let (t1, t2) = (ts[0], ts[1]);
        // rotate t1 so it reads (a, b, c) with edge a-b == e
        let tri = |t: usize| [tris[3 * t], tris[3 * t + 1], tris[3 * t + 2]];
        let rot = |t: [usize; 3], x: usize| -> [usize; 3] {
            let i = t.iter().position(|v| *v == x).unwrap_or(0);
            [t[i], t[(i + 1) % 3], t[(i + 2) % 3]]
        };
        let a1 = tri(t1);
        let r1 = if key(a1[0], a1[1]) == e { a1 } else if key(a1[1], a1[2]) == e { rot(a1, a1[1]) } else { rot(a1, a1[2]) };
        let (a, b, c) = (r1[0], r1[1], r1[2]);
        let r2 = rot(tri(t2), b); // (b, a, d)
        if r2[1] != a {
            continue;
        }
        let d = r2[2];
        if c == d || !in_circle(p(a), p(b), p(c), p(d)) {
            continue;
        }
        // the flip is valid only for a strictly convex quad a-d-b-c
        if orient(p(a), p(d), p(c)) <= 0.0 || orient(p(d), p(b), p(c)) <= 0.0 {
            continue;
        }
        tris[3 * t1..3 * t1 + 3].copy_from_slice(&[a, d, c]);
        tris[3 * t2..3 * t2 + 3].copy_from_slice(&[d, b, c]);
        edges.remove(&e);
        edges.insert(key(c, d), vec![t1, t2]);
        // edges b-c moved from t1 to t2, a-d moved from t2 to t1
        if let Some(v) = edges.get_mut(&key(b, c)) {
            for t in v.iter_mut() {
                if *t == t1 {
                    *t = t2;
                }
            }
        }
        if let Some(v) = edges.get_mut(&key(a, d)) {
            for t in v.iter_mut() {
                if *t == t2 {
                    *t = t1;
                }
            }
        }
        for k in [key(a, c), key(b, c), key(a, d), key(b, d)] {
            queue.push(k);
        }
    }
}

/// Add a triangle, wound so its geometric normal agrees with the vertex normals.
fn emit_triangle(out: &mut Tess, a: u32, b: u32, c: u32) {
    let (pa, pb, pc) = (out.positions[a as usize], out.positions[b as usize], out.positions[c as usize]);
    let g = cross(sub(pb, pa), sub(pc, pa));
    if len(g) <= 1e-300 {
        return;
    }
    let ns = add(add(out.normals[a as usize], out.normals[b as usize]), out.normals[c as usize]);
    let (b, c) = if dot(g, ns) < 0.0 { (c, b) } else { (b, c) };
    // fill missing normals (singular points) from the face
    let gn = norm(if dot(g, ns) < 0.0 { mul(g, -1.0) } else { g });
    for i in [a, b, c] {
        if len(out.normals[i as usize]) < 0.5 {
            out.normals[i as usize] = gn;
        }
    }
    out.indices.extend_from_slice(&[a, b, c]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::acis::primitives::*;
    use std::f64::consts::PI;

    /// Enclosed volume and surface area of a closed triangle mesh.
    fn volume_area(t: &Tess) -> (f64, f64) {
        let (mut vol, mut area) = (0.0, 0.0);
        for tri in t.indices.chunks_exact(3) {
            let (a, b, c) = (t.positions[tri[0] as usize], t.positions[tri[1] as usize], t.positions[tri[2] as usize]);
            area += len(cross(sub(b, a), sub(c, a))) / 2.0;
            vol += dot(a, cross(b, c)) / 6.0;
        }
        (vol, area)
    }

    fn check(doc: SatDocument, volume: f64, area: f64) {
        let doc = SatDocument::parse(&doc.to_sat_string()).unwrap();
        let mut t = Tess::default();
        tessellate(&doc, &mut t);
        assert_eq!(t.faces_failed, 0);
        let (v, a) = volume_area(&t);
        // outward normals give a positive volume; facets make it slightly smaller
        assert!((v / volume - 1.0).abs() < 0.03, "volume {v} expected {volume}");
        assert!((a / area - 1.0).abs() < 0.03, "area {a} expected {area}");
    }

    #[test]
    fn primitives_are_closed_and_outward() {
        check(build_box([1.0, 2.0, 3.0], 2.0, 3.0, 4.0), 24.0, 52.0);
        check(build_cylinder([0.0, 0.0, 0.0], 1.0, 2.0), 2.0 * PI, 6.0 * PI);
        check(build_cone([0.0, 0.0, 0.0], 1.0, 2.0), 2.0 * PI / 3.0, PI + PI * 5f64.sqrt());
        check(build_sphere([0.5, 0.0, 0.0], 1.5), 4.5 * PI, 9.0 * PI);
        check(build_torus([0.0, 0.0, 1.0], 3.0, 1.0), 6.0 * PI * PI, 12.0 * PI * PI);
    }

    #[test]
    fn delaunay_flips_untangle_ear_clipping_fans() {
        // a band with points only on its long sides: ear clipping makes two big fans
        let mut coords = Vec::new();
        for i in 0..=36 {
            coords.extend_from_slice(&[i as f64 * 0.1745, 0.0]);
        }
        for i in (0..=36).rev() {
            coords.extend_from_slice(&[i as f64 * 0.1745, 2.0]);
        }
        let mut tris = earcutr::earcut(&coords, &[], 2).unwrap();
        delaunay_flips(&coords, &mut tris);
        let widest = tris
            .chunks_exact(3)
            .map(|t| {
                let xs = t.iter().map(|&i| coords[2 * i]);
                xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min)
            })
            .fold(0.0, f64::max);
        assert!(widest < 0.2, "triangle spans {widest}");
    }
}
