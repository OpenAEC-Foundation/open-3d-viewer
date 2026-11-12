//! The shapes of a coordination file as triangles.
//!
//! An element's shapes stand in its coordinate system, which stands in its storey's, and so on
//! up to the project: a shape's place in the world is the product of that chain. A shape is
//! faces as loops of points, a profile drawn along a line, several shapes as one, one shape cut
//! by another (drawn uncut), or another shape's form at its own place (the file says at its
//! end which shape is whose; the form comes without where that other shape stands). The parts of
//! a compound shape stand in that shape's frame. Lengths in the file are millimetres; triangles come in metres.

use super::stream::{Data, Elem, GeoKind, V};
use glam::{DMat4, DVec2, DVec3};
use std::collections::HashMap;

/// Triangles, flat shaded: the corners of each face once, with the face's normal.
#[derive(Default)]
pub struct Mesh {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub colors: Vec<[u8; 4]>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn clear(&mut self) {
        self.positions.clear();
        self.normals.clear();
        self.colors.clear();
        self.indices.clear();
    }

    /// The corners of a face; gives the number of the first.
    fn corners(&mut self, points: &[DVec3], normal: DVec3, color: [u8; 4]) -> u32 {
        let first = (self.positions.len() / 3) as u32;
        for p in points {
            self.positions.extend([(p.x * 0.001) as f32, (p.y * 0.001) as f32, (p.z * 0.001) as f32]);
            self.normals.extend([normal.x as f32, normal.y as f32, normal.z as f32]);
            self.colors.push(color);
        }
        first
    }
}

const GREY: [u8; 4] = [184, 184, 189, 255];
/// How deep shapes of shapes are followed.
const DEEPEST: u32 = 12;

pub struct Shapes<'a> {
    d: &'a Data,
    world: Vec<Option<DMat4>>,
    /// The shape whose form a mapped shape takes.
    source: HashMap<u32, u32>,
    /// Shapes left out: mapped ones without a form, and cuts drawn uncut.
    pub formless: usize,
    pub uncut: usize,
}

impl<'a> Shapes<'a> {
    pub fn new(d: &'a Data) -> Self {
        let mut source = HashMap::new();
        for rec in d.recs.iter().filter(|r| &*d.classes[r.class as usize].short == "IsMappedBy") {
            let V::Geo(from) = d.field(rec, "Item", "parent") else { continue };
            for item in d.list(d.field(rec, "Property", "value")) {
                if let V::Geo(mapped) = item {
                    source.insert(*mapped, from);
                }
            }
        }
        Shapes { d, world: vec![None; d.csys.len()], source, formless: 0, uncut: 0 }
    }

    fn matrix(&self, v: V) -> DMat4 {
        match v {
            V::Mat(k) => DMat4::from_cols_array(&self.d.mats[k as usize]).transpose(),
            _ => DMat4::IDENTITY,
        }
    }

    /// Where a coordinate system (or a shape taken as one) stands in the world.
    fn place(&mut self, v: V, depth: u32) -> DMat4 {
        if depth > 64 {
            return DMat4::IDENTITY;
        }
        match v {
            V::Csys(k) => {
                if let Some(m) = self.world[k as usize] {
                    return m;
                }
                let (matrix, origin) = self.d.csys[k as usize];
                let m = self.place(origin, depth + 1) * self.matrix(matrix);
                self.world[k as usize] = Some(m);
                m
            }
            V::Geo(k) => {
                let g = &self.d.geos[k as usize];
                let (origin, matrix) = (g.origin, g.matrix);
                self.place(origin, depth + 1) * self.matrix(matrix)
            }
            _ => DMat4::IDENTITY,
        }
    }

    /// Where a part of a compound shape stands in that shape's frame: by its own matrix when
    /// it names no system to stand in, else as the two stand in the world.
    fn beside(&mut self, whole: u32, part: u32) -> DMat4 {
        let g = &self.d.geos[part as usize];
        match g.origin {
            V::Csys(_) | V::Geo(_) => self.place(V::Geo(whole), 0).inverse() * self.place(V::Geo(part), 0),
            _ => self.matrix(g.matrix),
        }
    }

    /// For checking the reader against the file: a compound shape's own faces, and its parts
    /// put together, both in the shape's own frame.
    #[cfg(test)]
    pub fn own_and_parts(&mut self, k: u32) -> Option<(Mesh, Mesh)> {
        let g = &self.d.geos[k as usize];
        let slots = g.slots;
        if g.kind != GeoKind::Multi || !matches!(slots[0], V::F32s(n) if !self.d.f32s[n as usize].is_empty()) || self.d.list(slots[4]).is_empty() {
            return None;
        }
        let (mut own, mut parts) = (Mesh::default(), Mesh::default());
        self.faces(slots, DMat4::IDENTITY, GREY, &mut own);
        for item in self.d.list(slots[4]) {
            if let V::Geo(inner) = *item {
                let there = self.beside(k, inner);
                self.shape(inner, there, GREY, 1, &mut parts);
            }
        }
        Some((own, parts))
    }

    /// The triangles of an element's shapes.
    pub fn element(&mut self, e: &Elem, out: &mut Mesh) {
        for child in self.d.list(e.children) {
            if let V::Geo(k) = *child {
                let at = self.place(V::Geo(k), 0);
                self.shape(k, at, GREY, 0, out);
            }
        }
    }

    /// A shape with its own frame at `at`.
    fn shape(&mut self, k: u32, at: DMat4, color: [u8; 4], depth: u32, out: &mut Mesh) {
        if depth > DEEPEST {
            return;
        }
        let g = &self.d.geos[k as usize];
        let (kind, slots) = (g.kind, g.slots);
        let color = match g.color {
            V::Color(c) => c.to_be_bytes(),
            _ => color,
        };
        let part = |shapes: &mut Self, part: u32| at * shapes.beside(k, part);
        match kind {
            GeoKind::Brep => self.faces(slots, at, color, out),
            GeoKind::Multi => {
                if matches!(slots[0], V::F32s(n) if !self.d.f32s[n as usize].is_empty()) {
                    self.faces(slots, at, color, out);
                } else {
                    for item in self.d.list(slots[4]) {
                        if let V::Geo(inner) = *item {
                            let there = part(self, inner);
                            self.shape(inner, there, color, depth + 1, out);
                        }
                    }
                }
            }
            GeoKind::Mapped => match self.source.get(&k).copied() {
                Some(from) => {
                    // the form alone: where the other shape stands itself does not come along
                    self.shape(from, at, color, depth + 1, out);
                }
                None => self.formless += 1,
            },
            GeoKind::Extrusion => self.extrusion(slots, at, color, out),
            GeoKind::Csg => {
                self.uncut += 1;
                if let V::Geo(first) = slots[0] {
                    let there = part(self, first);
                    self.shape(first, there, color, depth + 1, out);
                }
            }
            GeoKind::Halfspace | GeoKind::Other => {}
        }
    }

    /// Faces as loops of points: `slots` are the points, the size of each loop and the loops of
    /// each face (the first its outline, the others holes).
    fn faces(&mut self, slots: [V; 5], at: DMat4, color: [u8; 4], out: &mut Mesh) {
        let (V::F32s(v), V::I32s(s)) = (slots[0], slots[1]) else { return };
        let (vertices, sizes) = (&self.d.f32s[v as usize], &self.d.i32s[s as usize]);
        let per_face: &[i32] = match slots[2] {
            V::I32s(c) => &self.d.i32s[c as usize],
            _ => &[],
        };
        let point = |i: usize| vertices.get(i * 3..i * 3 + 3).map(|p| at.transform_point3(DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64)));
        let (mut next, mut loop_at) = (0usize, 0usize);
        let mut face = 0;
        let mut loops: Vec<Vec<DVec3>> = Vec::new();
        while loop_at < sizes.len() {
            let count = per_face.get(face).copied().unwrap_or(1).max(1) as usize;
            loops.clear();
            for size in sizes[loop_at..].iter().take(count) {
                let size = (*size).max(0) as usize;
                loops.push((next..next + size).filter_map(point).collect());
                next += size;
            }
            loop_at += count;
            face += 1;
            polygon(&loops, color, out);
        }
    }

    /// A profile drawn along a line: `slots` are the profile and the two ends of the line.
    fn extrusion(&mut self, slots: [V; 5], at: DMat4, color: [u8; 4], out: &mut Mesh) {
        let d = self.d;
        let V::Rec(p) = slots[0] else { return };
        let profile = &d.recs[p as usize];
        let ends: Vec<DVec3> = d.list(slots[1]).iter().filter_map(|v| if let V::Pt(k) = v { Some(DVec3::from(d.points[*k as usize].map(f64::from))) } else { None }).collect();
        let [from, to] = ends[..] else { return };
        let flat = |v: V| -> Vec<DVec2> { d.list(v).iter().filter_map(|v| if let V::Pt(k) = v { Some(DVec2::new(d.points[*k as usize][0] as f64, d.points[*k as usize][1] as f64)) } else { None }).collect() };
        let mut loops: Vec<Vec<DVec2>> = Vec::new();
        let outline = flat(d.any_field(profile, "outerCurve"));
        if !outline.is_empty() {
            loops.push(outline);
            loops.extend(d.list(d.any_field(profile, "innerCurves")).iter().map(|v| flat(*v)).filter(|l| l.len() >= 3));
        } else {
            let numbers: Vec<f64> = d.list(d.any_field(profile, "parameters")).iter().map(|v| if let V::Num(x) = v { *x as f64 } else { 0.0 }).collect();
            loops = parametric(&numbers);
        }
        // a loop that ends where it starts says its first point twice
        for l in &mut loops {
            if l.len() > 3 && l.first().zip(l.last()).is_some_and(|(a, b)| a.distance(*b) < 1e-6) {
                l.pop();
            }
        }
        if loops.first().is_none_or(|l| l.len() < 3) {
            return;
        }
        let own = self.matrix(d.any_field(profile, "transform"));
        let lift = |p: DVec2, end: DVec3| at.transform_point3(own.transform_point3(DVec3::new(p.x, p.y, 0.0)) + end);
        let (bottom, top): (Vec<Vec<DVec3>>, Vec<Vec<DVec3>>) = (loops.iter().map(|l| l.iter().map(|p| lift(*p, from)).collect()).collect(), loops.iter().map(|l| l.iter().map(|p| lift(*p, to)).collect()).collect());
        polygon(&bottom, color, out);
        polygon(&top, color, out);
        for (low, high) in bottom.iter().zip(&top) {
            for i in 0..low.len() {
                let j = (i + 1) % low.len();
                polygon(&[vec![low[i], low[j], high[j], high[i]]], color, out);
            }
        }
    }
}

/// The outline of a profile given by its measures. The number of measures tells the kind: one
/// is a circle's radius, two a rectangle, five a hollow rectangle, seven an I shape; the others
/// (slabs with cores, built-up sections) are drawn as the rectangle around them.
fn parametric(n: &[f64]) -> Vec<Vec<DVec2>> {
    let rectangle = |x: f64, y: f64| vec![DVec2::new(-x / 2.0, -y / 2.0), DVec2::new(x / 2.0, -y / 2.0), DVec2::new(x / 2.0, y / 2.0), DVec2::new(-x / 2.0, y / 2.0)];
    let get = |k: usize| n.get(k).copied().unwrap_or(0.0);
    match n.len() {
        0 => Vec::new(),
        1 => vec![(0..24).map(|k| DVec2::from_angle(k as f64 * std::f64::consts::TAU / 24.0) * get(0)).collect()],
        5 if get(2) > 0.0 && get(2) * 2.0 < get(0).min(get(1)) => vec![rectangle(get(0), get(1)), rectangle(get(0) - 2.0 * get(2), get(1) - 2.0 * get(2))],
        7 if get(2) > 0.0 && get(3) > 0.0 => {
            let (b, h, web, flange) = (get(0) / 2.0, get(1) / 2.0, get(2) / 2.0, get(3));
            let xy = [(-b, -h), (b, -h), (b, -h + flange), (web, -h + flange), (web, h - flange), (b, h - flange), (b, h), (-b, h), (-b, h - flange), (-web, h - flange), (-web, -h + flange), (-b, -h + flange)];
            vec![xy.iter().map(|&(x, y)| DVec2::new(x, y)).collect()]
        }
        9.. => vec![rectangle(get(1), get(0))],
        _ => vec![rectangle(get(0), get(1))],
    }
}

/// A flat face with holes, as triangles.
fn polygon(loops: &[Vec<DVec3>], color: [u8; 4], out: &mut Mesh) {
    let Some(outline) = loops.first().filter(|l| l.len() >= 3) else { return };
    // Newell's normal of the outline
    let mut normal = DVec3::ZERO;
    for (i, a) in outline.iter().enumerate() {
        let b = outline[(i + 1) % outline.len()];
        normal += DVec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
    }
    let Some(normal) = normal.try_normalize() else { return };
    if outline.len() == 3 && loops.len() == 1 {
        let first = out.corners(outline, normal, color);
        out.indices.extend([first, first + 1, first + 2]);
        return;
    }
    // flat: the two axes the face lies along most
    let drop = if normal.x.abs() >= normal.y.abs() && normal.x.abs() >= normal.z.abs() { 0 } else if normal.y.abs() >= normal.z.abs() { 1 } else { 2 };
    let (u, v) = [(1, 2), (2, 0), (0, 1)][drop];
    let points: Vec<DVec3> = loops.iter().flatten().copied().collect();
    let flat: Vec<f64> = points.iter().flat_map(|p| [p[u], p[v]]).collect();
    let mut holes = Vec::new();
    let mut at = 0;
    for l in &loops[..loops.len() - 1] {
        at += l.len();
        holes.push(at);
    }
    let Ok(triangles) = earcutr::earcut(&flat, &holes, 2) else { return };
    let first = out.corners(&points, normal, color);
    for t in triangles.chunks_exact(3) {
        let (a, b, c) = (points[t[0]], points[t[1]], points[t[2]]);
        // the corners turn as the face's normal says
        let turned = (b - a).cross(c - a).dot(normal) < 0.0;
        let (second, third) = if turned { (t[2], t[1]) } else { (t[1], t[2]) };
        out.indices.extend([first + t[0] as u32, first + second as u32, first + third as u32]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_face_with_a_hole_keeps_the_hole_open() {
        let square = |size: f64| vec![DVec3::new(-size, -size, 0.0), DVec3::new(size, -size, 0.0), DVec3::new(size, size, 0.0), DVec3::new(-size, size, 0.0)];
        let mut out = Mesh::default();
        polygon(&[square(2000.0), square(1000.0)], GREY, &mut out);
        assert_eq!(out.triangles(), 8);
        // 16 m2 less the 4 m2 of the hole, every triangle facing up
        // the eight corners once, shared by the triangles
        assert_eq!(out.positions.len(), 8 * 3);
        let at = |i: u32| [out.positions[i as usize * 3], out.positions[i as usize * 3 + 1]];
        let area: f32 = out.indices.chunks_exact(3).map(|t| (at(t[0]), at(t[1]), at(t[2]))).map(|(a, b, c)| ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])) / 2.0).sum();
        assert!((area - 12.0).abs() < 1e-3, "{area}");
        assert!(out.normals.chunks_exact(3).all(|n| n == [0.0, 0.0, 1.0]));
    }

    #[test]
    fn the_measures_of_a_profile_give_its_outline() {
        assert_eq!(parametric(&[80.0])[0].len(), 24);
        assert_eq!(parametric(&[2605.0, 200.0])[0][2], DVec2::new(1302.5, 100.0));
        assert_eq!(parametric(&[180.0, 180.0, 10.0, 10.0, 20.0]).len(), 2);
        assert_eq!(parametric(&[240.0, 240.0, 10.0, 17.0, 21.0, 0.0, 0.0])[0].len(), 12);
        // a slab with cores: the rectangle around it, its width first
        assert_eq!(parametric(&[670.0, 1200.0, 900.0, 395.0, 0.0, 0.0, 0.0, 0.0, 0.0])[0][2], DVec2::new(600.0, 335.0));
    }
}
