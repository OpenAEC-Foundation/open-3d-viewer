//! Rhino SubD objects (`ON_SubD`, Rhino 7 and later). Rhino keeps no mesh of a SubD in the file:
//! it shows the limit surface of the control net. The viewer reads the control net (level 0) as
//! openNURBS writes it and subdivides it as openNURBS does: Catmull-Clark with Rhino's rules for
//! tagged vertices (crease, corner, dart) through the sector coefficients of their edges, its
//! weights for vertices among triangles, crease edges, and semi-sharp edges (Rhino 8) whose
//! sharpness falls by one per level. The last level's points are moved to the limit surface.
//! Normals are smooth across smooth edges and break at creases.

use super::{err, Cur, Mesh, R, TCODE_ANONYMOUS_CHUNK};
use glam::DVec3;
use std::collections::HashMap;

const SMOOTH: u8 = 1;
const CREASE: u8 = 2;
const CORNER: u8 = 3;
const DART: u8 = 4;
/// The edge tag of a smooth edge whose sector coefficients are ignored.
const SMOOTH_X: u8 = 4;

/// Most faces of the subdivided net drawn for one SubD.
const MAX_FACES: usize = 120_000;
/// At most this many vertices, edges or faces in a control net.
const MAX_COMPONENTS: usize = 2_000_000;

#[derive(Clone, Copy)]
struct Edge {
    v: [u32; 2],
    tag: u8,
    /// The sector coefficient at each end, for a smooth edge to a tagged vertex.
    coefficient: [f64; 2],
    /// The sharpness at each end of a smooth edge (Rhino 8); 0 when not sharp.
    sharpness: [f64; 2],
}

impl Edge {
    fn smooth(&self) -> bool {
        self.tag == SMOOTH || self.tag == SMOOTH_X || self.tag == 0
    }

    fn sharp(&self) -> bool {
        self.smooth() && (self.sharpness[0] > 0.0 || self.sharpness[1] > 0.0)
    }

    fn other(&self, v: u32) -> u32 {
        if self.v[0] == v {
            self.v[1]
        } else {
            self.v[0]
        }
    }

    fn end_sharpness(&self, v: u32) -> f64 {
        if !self.smooth() {
            0.0
        } else if self.v[0] == v {
            self.sharpness[0]
        } else {
            self.sharpness[1]
        }
    }
}

/// A control net: vertices, edges and faces as loops of vertices.
#[derive(Default)]
struct Net {
    points: Vec<DVec3>,
    tags: Vec<u8>,
    edges: Vec<Edge>,
    /// Edge by its vertices (smaller first).
    by_ends: HashMap<(u32, u32), u32>,
    faces: Vec<Vec<u32>>,
    colors: Vec<Option<[u8; 4]>>,
}

/// What each vertex and edge touches.
struct Adjacency {
    vertex_edges: Vec<Vec<u32>>,
    vertex_faces: Vec<Vec<u32>>,
    edge_faces: Vec<Vec<u32>>,
}

fn key(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

impl Net {
    fn edge(&self, a: u32, b: u32) -> Option<u32> {
        self.by_ends.get(&key(a, b)).copied()
    }

    fn adjacency(&self) -> Adjacency {
        let mut vertex_edges = vec![Vec::new(); self.points.len()];
        let mut vertex_faces = vec![Vec::new(); self.points.len()];
        let mut edge_faces = vec![Vec::new(); self.edges.len()];
        for (k, e) in self.edges.iter().enumerate() {
            for &v in &e.v {
                vertex_edges[v as usize].push(k as u32);
            }
        }
        for (f, face) in self.faces.iter().enumerate() {
            for (i, &v) in face.iter().enumerate() {
                vertex_faces[v as usize].push(f as u32);
                if let Some(e) = self.edge(v, face[(i + 1) % face.len()]) {
                    edge_faces[e as usize].push(f as u32);
                }
            }
        }
        Adjacency { vertex_edges, vertex_faces, edge_faces }
    }

    fn centroid(&self, f: usize) -> DVec3 {
        let face = &self.faces[f];
        face.iter().map(|&v| self.points[v as usize]).sum::<DVec3>() / face.len() as f64
    }

    /// `ON_SubDVertex::GetSharpSubdivisionPoint`: the vertex sharpness and the point it pulls to.
    fn sharp_point(&self, v: u32, adj: &Adjacency) -> (f64, DVec3) {
        let tag = self.tags[v as usize];
        let crease_edges = match tag {
            SMOOTH => 0,
            CREASE => 2,
            DART => 1,
            _ => return (0.0, DVec3::NAN),
        };
        let mut sharp_count = 0;
        let mut max_sharpness: f64 = 0.0;
        let mut others = Vec::new();
        for &e in &adj.vertex_edges[v as usize] {
            let edge = &self.edges[e as usize];
            if edge.tag == CREASE {
                others.push(edge.other(v));
            } else if edge.sharp() {
                let s = edge.end_sharpness(v);
                if s > 0.0 {
                    others.push(edge.other(v));
                    max_sharpness = max_sharpness.max(s);
                    sharp_count += 1;
                }
            }
        }
        if sharp_count + crease_edges < 2 || max_sharpness <= 0.0 {
            return (0.0, DVec3::NAN);
        }
        let p = self.points[v as usize];
        let point = if sharp_count > 0 && others.len() == 2 { 0.75 * p + 0.125 * (self.points[others[0] as usize] + self.points[others[1] as usize]) } else { p };
        (max_sharpness, point)
    }

    fn vertex_point(&self, v: u32, adj: &Adjacency) -> DVec3 {
        let p = self.points[v as usize];
        let edges = &adj.vertex_edges[v as usize];
        let faces = &adj.vertex_faces[v as usize];
        let blend = |smooth: DVec3, (s, sharp): (f64, DVec3)| if s >= 1.0 { sharp } else if s > 0.0 { (1.0 - s) * smooth + s * sharp } else { smooth };
        match self.tags[v as usize] {
            CORNER => p,
            CREASE => {
                let creases: Vec<u32> = edges.iter().map(|&e| &self.edges[e as usize]).filter(|e| e.tag == CREASE).map(|e| e.other(v)).collect();
                if creases.len() != 2 {
                    return p;
                }
                let smooth = (6.0 * p + self.points[creases[0] as usize] + self.points[creases[1] as usize]) * 0.125;
                let sharp = if edges.iter().any(|&e| self.edges[e as usize].sharp()) { self.sharp_point(v, adj) } else { (0.0, DVec3::NAN) };
                blend(smooth, sharp)
            }
            _ => {
                let n = edges.len();
                if n < 2 || faces.len() != n {
                    return p;
                }
                let n_f = n as f64;
                let edge_sum: DVec3 = edges.iter().map(|&e| self.points[self.edges[e as usize].other(v) as usize]).sum();
                let smooth = if faces.iter().all(|&f| self.faces[f as usize].len() == 3) {
                    // Rhino's weights for a vertex among triangles
                    (1.0 - 5.0 / (3.0 * n_f)) * p + 5.0 / (3.0 * n_f * n_f) * edge_sum
                } else {
                    let face_sum: DVec3 = faces.iter().map(|&f| self.centroid(f as usize)).sum();
                    (1.0 - 2.0 / n_f) * p + (edge_sum + face_sum) / (n_f * n_f)
                };
                blend(smooth, self.sharp_point(v, adj))
            }
        }
    }

    fn edge_point(&self, e: usize, adj: &Adjacency) -> DVec3 {
        let edge = &self.edges[e];
        let p = [self.points[edge.v[0] as usize], self.points[edge.v[1] as usize]];
        let faces = &adj.edge_faces[e];
        if !edge.smooth() || faces.len() != 2 {
            return 0.5 * (p[0] + p[1]);
        }
        let tags = edge.v.map(|v| self.tags[v as usize]);
        let tagged = if tags[0] != SMOOTH { Some(0) } else if tags[1] != SMOOTH { Some(1) } else { None };
        let ep = match tagged {
            Some(t) if edge.tag != SMOOTH_X && edge.coefficient[t] != 0.5 && tags[1 - t] == SMOOTH && edge.coefficient[t] > 0.0 && edge.coefficient[t] < 1.0 => {
                let mut w = [0.0; 2];
                w[t] = edge.coefficient[t];
                w[1 - t] = 1.0 - w[t];
                0.75 * (w[0] * p[0] + w[1] * p[1])
            }
            _ => 0.375 * (p[0] + p[1]),
        };
        let mut point = ep - 0.125 * (p[0] + p[1]);
        for &f in faces {
            let face = &self.faces[f as usize];
            let n = face.len() as f64;
            let sum: DVec3 = face.iter().map(|&v| self.points[v as usize]).sum();
            point += sum / (4.0 * n);
        }
        if edge.sharp() {
            let s = 0.5 * (edge.sharpness[0] + edge.sharpness[1]);
            let mid = 0.5 * (p[0] + p[1]);
            point = if s >= 1.0 { mid } else { (1.0 - s) * point + s * mid };
        }
        point
    }

    /// One Catmull-Clark step: every face of n sides becomes n quads.
    fn subdivide(&self) -> Net {
        let adj = self.adjacency();
        let nv = self.points.len();
        let ne = self.edges.len();
        let mut next = Net::default();
        // vertex points, then edge points, then face points
        for v in 0..nv {
            next.points.push(self.vertex_point(v as u32, &adj));
            next.tags.push(self.tags[v]);
        }
        for e in 0..ne {
            next.points.push(self.edge_point(e, &adj));
            next.tags.push(if self.edges[e].smooth() && adj.edge_faces[e].len() == 2 { SMOOTH } else { CREASE });
        }
        for f in 0..self.faces.len() {
            next.points.push(self.centroid(f));
            next.tags.push(SMOOTH);
        }
        let edge_vertex = |e: u32| (nv as u32) + e;
        let face_vertex = |f: usize| (nv + ne + f) as u32;
        let add = |next: &mut Net, edge: Edge| {
            let k = key(edge.v[0], edge.v[1]);
            if !next.by_ends.contains_key(&k) {
                next.by_ends.insert(k, next.edges.len() as u32);
                next.edges.push(edge);
            }
        };
        let sanitize = |s: f64| if s > 0.0 { s } else { 0.0 };
        for (e, edge) in self.edges.iter().enumerate() {
            let mid = edge_vertex(e as u32);
            let [s0, s1] = edge.sharpness.map(|s| sanitize(s - 1.0));
            let s_mid = 0.5 * (s0 + s1);
            let tag = if edge.tag == SMOOTH_X { SMOOTH } else { edge.tag };
            add(&mut next, Edge { v: [edge.v[0], mid], tag, coefficient: [edge.coefficient[0], 0.0], sharpness: [s0, s_mid] });
            add(&mut next, Edge { v: [mid, edge.v[1]], tag, coefficient: [0.0, edge.coefficient[1]], sharpness: [s_mid, s1] });
        }
        for (f, face) in self.faces.iter().enumerate() {
            let center = face_vertex(f);
            let n = face.len();
            let mids: Vec<Option<u32>> = (0..n).map(|i| self.edge(face[i], face[(i + 1) % n]).map(edge_vertex)).collect();
            if mids.iter().any(Option::is_none) {
                continue;
            }
            let mids: Vec<u32> = mids.into_iter().flatten().collect();
            for &m in &mids {
                add(&mut next, Edge { v: [m, center], tag: SMOOTH, coefficient: [0.0; 2], sharpness: [0.0; 2] });
            }
            for i in 0..n {
                next.faces.push(vec![face[i], mids[i], center, mids[(i + n - 1) % n]]);
                next.colors.push(self.colors[f]);
            }
        }
        next
    }

    /// The points of a net of quads moved to the limit surface: smooth vertices by the
    /// Catmull-Clark limit stencil, crease vertices along their crease, corners stay.
    fn limit_points(&self) -> Vec<DVec3> {
        let adj = self.adjacency();
        (0..self.points.len())
            .map(|v| {
                let p = self.points[v];
                if self.sharp_point(v as u32, &adj).0 > 0.0 {
                    return p;
                }
                let edges = &adj.vertex_edges[v];
                match self.tags[v] {
                    CORNER => p,
                    CREASE => {
                        let creases: Vec<u32> = edges.iter().map(|&e| &self.edges[e as usize]).filter(|e| e.tag == CREASE).map(|e| e.other(v as u32)).collect();
                        if creases.len() == 2 {
                            (self.points[creases[0] as usize] + 4.0 * p + self.points[creases[1] as usize]) / 6.0
                        } else {
                            p
                        }
                    }
                    _ => {
                        let faces = &adj.vertex_faces[v];
                        let n = edges.len();
                        if n < 3 || faces.len() != n || faces.iter().any(|&f| self.faces[f as usize].len() != 4) {
                            return p;
                        }
                        let n_f = n as f64;
                        let edge_sum: DVec3 = edges.iter().map(|&e| self.points[self.edges[e as usize].other(v as u32) as usize]).sum();
                        // the corner of each quad across from the vertex
                        let diagonal_sum: DVec3 = faces
                            .iter()
                            .map(|&f| {
                                let face = &self.faces[f as usize];
                                let i = face.iter().position(|&w| w as usize == v).unwrap_or(0);
                                self.points[face[(i + 2) % 4] as usize]
                            })
                            .sum();
                        (n_f * n_f * p + 4.0 * edge_sum + diagonal_sum) / (n_f * (n_f + 5.0))
                    }
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------------------------
// Reading: `ON_SubD::Read`, `ON_SubDimple::Read`, `ON_SubDLevel::Read` and the components.

const ADDITION_END: u8 = 255;
const ADDITION_CHUNK: u8 = 254;

/// An addition size (`Internal_ReadComponentAdditionSize`): 0, the expected size, or the end mark.
fn addition(c: &mut Cur<'_>, size: u8) -> R<u8> {
    let sz = c.u8()?;
    if sz != 0 && sz != size && sz != ADDITION_END {
        return err("a SubD component addition of an unknown size");
    }
    Ok(sz)
}

/// `Internal_FinishReadingComponentAdditions`: skips the additions of later versions.
fn finish_additions(c: &mut Cur<'_>) -> R<()> {
    let mut sz = c.u8()?;
    loop {
        match sz {
            ADDITION_END => return Ok(()),
            ADDITION_CHUNK => {
                c.chunk()?;
            }
            0 => {}
            n => {
                c.take(n as usize)?;
            }
        }
        sz = c.u8()?;
    }
}

/// `ReadBase`: archive id, id and level, and (Rhino 7 and later) the additions; also whether
/// they ended early (the end mark).
fn read_base(c: &mut Cur<'_>) -> R<(u32, bool)> {
    let archive_id = c.u32()?;
    c.take(4 + 2)?; // id, level
    if c.v.archive < 70 {
        if c.u8()? != 0 {
            c.take(24)?;
        }
        if c.u8()? != 0 {
            c.take(24)?;
        }
        return Ok((archive_id, false));
    }
    for size in [24u8, 4, 5] {
        match addition(c, size)? {
            ADDITION_END => return Ok((archive_id, true)),
            0 => {}
            n => {
                c.take(n as usize)?;
            }
        }
    }
    finish_additions(c)?;
    Ok((archive_id, false))
}

/// A list of component references: a count (u16) and, per item, an archive id and flags.
fn read_refs(c: &mut Cur<'_>) -> R<Vec<(u32, u8)>> {
    let n = c.take(2)?;
    let n = u16::from_le_bytes([n[0], n[1]]) as usize;
    if n * 5 > c.left() {
        return err("a SubD component list larger than its record");
    }
    (0..n).map(|_| Ok((c.u32()?, c.u8()? & 7))).collect()
}

/// The mesh of a SubD object's limit surface. `None` for an empty SubD.
pub(super) fn subd(mut c: Cur<'_>, vertices: &mut usize) -> R<Option<Mesh>> {
    if c.u8()? != 1 {
        return Ok(None);
    }
    let (mut s, major, _) = c.versioned(TCODE_ANONYMOUS_CHUNK)?;
    if major != 1 {
        return err("a SubD of an unknown version");
    }
    let levels = s.u32()?;
    if levels == 0 {
        return Ok(None);
    }
    s.take(12 + 48)?; // largest ids, bounding box
    // level 0, the control net
    let (mut l, major, _) = s.versioned(TCODE_ANONYMOUS_CHUNK)?;
    if major != 1 {
        return err("a SubD level of an unknown version");
    }
    l.take(2 + 3 + 48)?; // level index, subdivision type, bounding box
    let partition = [l.u32()?, l.u32()?, l.u32()?, l.u32()?];
    if partition.windows(2).any(|w| w[1] < w[0]) || (partition[3] - partition[0]) as usize > MAX_COMPONENTS {
        return err("a SubD with a damaged component count");
    }
    let mut net = Net::default();
    // vertices
    let mut vertex_index: HashMap<u32, u32> = HashMap::new();
    for _ in partition[0]..partition[1] {
        let (id, _) = read_base(&mut l)?;
        let tag = l.u8()?;
        let p = DVec3::new(l.f64()?, l.f64()?, l.f64()?);
        l.take(4)?; // edge and face counts
        if l.u8()? != 0 {
            // saved limit points, never used
            let n = l.count(101)?;
            l.take(101 * n)?;
        }
        read_refs(&mut l)?;
        read_refs(&mut l)?;
        if l.v.archive < 70 {
            l.u8()?;
        } else {
            finish_additions(&mut l)?;
        }
        if !p.is_finite() {
            return err("a SubD vertex that is not a number");
        }
        vertex_index.insert(id, net.points.len() as u32);
        net.points.push(p);
        net.tags.push(tag);
    }
    // edges
    let mut edge_index: HashMap<u32, u32> = HashMap::new();
    for _ in partition[1]..partition[2] {
        let (id, _) = read_base(&mut l)?;
        let tag = l.u8()?;
        l.take(2)?; // face count
        let coefficient = [l.f64()?, l.f64()?];
        let s0 = l.f64()?;
        let ends = read_refs(&mut l)?;
        read_refs(&mut l)?;
        let mut sharpness = [s0, s0];
        if l.v.archive < 70 {
            l.u8()?;
            sharpness = [0.0; 2];
        } else if l.v.archive >= 80 {
            match l.u8()? {
                ADDITION_END => {}
                8 => {
                    sharpness[1] = l.f64()?;
                    finish_additions(&mut l)?;
                }
                _ => return err("a SubD edge addition of an unknown size"),
            }
        } else {
            finish_additions(&mut l)?;
        }
        let v = [ends.first(), ends.get(1)].map(|r| r.and_then(|(id, _)| vertex_index.get(id).copied()));
        let (Some(a), Some(b)) = (v[0], v[1]) else { return err("a SubD edge without its vertices") };
        let sharpness = sharpness.map(|s| if s.is_finite() && s > 0.0 { s.min(16.0) } else { 0.0 });
        edge_index.insert(id, net.edges.len() as u32);
        net.by_ends.insert(key(a, b), net.edges.len() as u32);
        net.edges.push(Edge { v: [a, b], tag, coefficient, sharpness });
    }
    // faces
    for _ in partition[2]..partition[3] {
        read_base(&mut l)?;
        l.take(8)?; // level zero face id, parent face id
        l.take(2)?; // edge count
        let edges = read_refs(&mut l)?;
        let mut color = None;
        if l.v.archive < 70 {
            l.u8()?;
        } else {
            // texture domain, material channel, colour, pack id, texture points
            'additions: {
                for (k, size) in [34u8, 4, 4, 4, 4].into_iter().enumerate() {
                    match addition(&mut l, size)? {
                        ADDITION_END => break 'additions,
                        0 => {}
                        n => {
                            let bytes = l.take(n as usize)?;
                            if k == 2 {
                                color = Some([bytes[0], bytes[1], bytes[2], bytes[3]]).filter(|c| *c != [255; 4]);
                            }
                            if k == 4 {
                                // texture points: 10-point chunks and the rest, each with a size
                                let chunks = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
                                let rest = edges.len() % 10;
                                for _ in 0..chunks + usize::from(rest > 0) {
                                    let sz = l.u8()?;
                                    l.take(sz as usize)?;
                                }
                            }
                        }
                    }
                }
                finish_additions(&mut l)?;
            }
        }
        // the face's vertices, from its edges and their directions
        let mut face = Vec::with_capacity(edges.len());
        for (id, flags) in &edges {
            let Some(&e) = edge_index.get(id) else { return err("a SubD face without its edges") };
            let edge = &net.edges[e as usize];
            face.push(edge.v[(flags & 1) as usize]);
        }
        if face.len() >= 3 {
            net.faces.push(face);
            net.colors.push(color);
        }
    }
    if net.faces.is_empty() {
        return Ok(None);
    }
    // subdivide while the faces fit: about four times as many per level
    let mut level = 0;
    let mut faces = net.faces.iter().map(|f| f.len()).sum::<usize>();
    while level < 4 && faces * 4 <= MAX_FACES {
        net = net.subdivide();
        faces = net.faces.len();
        level += 1;
    }
    if level == 0 {
        net = net.subdivide();
    }
    let limit = net.limit_points();
    let mesh = mesh_of(&net, &limit);
    if mesh.positions.len() > super::MAX_VERTICES - *vertices {
        return Err(super::TOO_MANY_VERTICES.into());
    }
    *vertices += mesh.positions.len();
    Ok(Some(mesh))
}

/// Triangles of the quads, with normals smooth within about 35 degrees (creases stay sharp)
/// and the face colours when every face has one.
fn mesh_of(net: &Net, points: &[DVec3]) -> Mesh {
    let normal_of = |face: &Vec<u32>| {
        let p = |i: usize| points[face[i % face.len()] as usize];
        let mut n = DVec3::ZERO;
        for i in 0..face.len() {
            n += p(i).cross(p(i + 1));
        }
        n.normalize_or_zero()
    };
    let face_normals: Vec<DVec3> = net.faces.iter().map(normal_of).collect();
    let mut vertex_faces = vec![Vec::new(); points.len()];
    for (f, face) in net.faces.iter().enumerate() {
        for &v in face {
            vertex_faces[v as usize].push(f);
        }
    }
    let all_colored = net.colors.iter().all(Option::is_some);
    let cos_limit = 35f64.to_radians().cos();
    let mut mesh = Mesh::default();
    let mut colors = Vec::new();
    let mut normals = Vec::new();
    for (f, face) in net.faces.iter().enumerate() {
        let base = mesh.positions.len() as u32;
        let nf = face_normals[f];
        for &v in face {
            let mut n = DVec3::ZERO;
            for &g in &vertex_faces[v as usize] {
                if face_normals[g].dot(nf) >= cos_limit {
                    n += face_normals[g];
                }
            }
            let n = if n == DVec3::ZERO { nf } else { n.normalize_or_zero() };
            mesh.positions.push(points[v as usize].to_array().map(|x| x as f32));
            normals.push(n.to_array().map(|x| x as f32));
            if all_colored {
                colors.push(net.colors[f].unwrap_or([0; 4]));
            }
        }
        match face.len() {
            3 => mesh.faces.push([base, base + 1, base + 2, base + 2]),
            4 => {
                // split along the shorter diagonal
                let d0 = (points[face[0] as usize] - points[face[2] as usize]).length_squared();
                let d1 = (points[face[1] as usize] - points[face[3] as usize]).length_squared();
                if d0 <= d1 {
                    mesh.faces.push([base, base + 1, base + 2, base + 2]);
                    mesh.faces.push([base, base + 2, base + 3, base + 3]);
                } else {
                    mesh.faces.push([base, base + 1, base + 3, base + 3]);
                    mesh.faces.push([base + 1, base + 2, base + 3, base + 3]);
                }
            }
            n => {
                for i in 1..n as u32 - 1 {
                    mesh.faces.push([base, base + i, base + i + 1, base + i + 1]);
                }
            }
        }
    }
    mesh.triangles = mesh.faces.len();
    mesh.normals = Some(normals);
    mesh.colors = all_colored.then_some(colors);
    mesh.computed = true;
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cube as a control net of six quads, all smooth.
    fn cube(tag: u8, edge_tag: u8) -> Net {
        let mut net = Net::default();
        for i in 0..8 {
            net.points.push(DVec3::new((i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64) * 2.0 - DVec3::ONE);
            net.tags.push(tag);
        }
        let faces = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        for f in faces {
            for i in 0..4 {
                let (a, b) = (f[i], f[(i + 1) % 4]);
                if !net.by_ends.contains_key(&key(a, b)) {
                    net.by_ends.insert(key(a, b), net.edges.len() as u32);
                    net.edges.push(Edge { v: [a, b], tag: edge_tag, coefficient: [0.0; 2], sharpness: [0.0; 2] });
                }
            }
            net.faces.push(f.to_vec());
            net.colors.push(None);
        }
        net
    }

    #[test]
    fn a_smooth_cube_rounds_into_a_blob_inside_its_cage() {
        let mut net = cube(SMOOTH, SMOOTH);
        for _ in 0..3 {
            net = net.subdivide();
        }
        assert_eq!(net.faces.len(), 6 * 64);
        let limit = net.limit_points();
        for p in &limit {
            let r = p.length();
            // the Catmull-Clark limit of a cube lies inside it, rounded off its corners
            assert!(r > 0.5 && r < 1.2, "{r}");
            assert!(p.abs().max_element() < 1.0);
        }
    }

    #[test]
    fn creased_corners_keep_the_cube() {
        let mut net = cube(CORNER, CREASE);
        for _ in 0..2 {
            net = net.subdivide();
        }
        // with every edge a crease and every vertex a corner the cube stays a cube
        for p in net.limit_points() {
            assert!((p.abs().max_element() - 1.0).abs() < 1e-12, "{p}");
        }
    }

    #[test]
    fn sharp_edges_soften_by_one_per_level() {
        let mut net = cube(SMOOTH, SMOOTH);
        for e in &mut net.edges {
            e.sharpness = [2.0, 2.0];
        }
        let sharp = net.subdivide().subdivide();
        assert!(sharp.edges.iter().all(|e| e.sharpness == [0.0; 2]), "sharpness 2 is gone after two levels");
        // sharper edges keep the corners further out than smooth ones
        let smooth = cube(SMOOTH, SMOOTH).subdivide().subdivide();
        let corner = |n: &Net| n.points[0].length();
        assert!(corner(&sharp) > corner(&smooth));
    }
}
