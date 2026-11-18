//! A terrain surface as a triangulated irregular network (TIN): points with a height, joined
//! to triangles.
//!
//! Built from measured points by a Delaunay triangulation. A breakline (the edge of a road, the
//! foot of a bank) is followed by the triangles: none crosses it. A boundary keeps the
//! triangles within an outline, and a longest edge takes away the long, thin triangles a
//! triangulation draws across the open sides of a survey. A grid of heights (a height model)
//! becomes a surface as well.
//!
//! A surface gives the height at a place, its contours, and volumes: against a level exactly,
//! against another surface by sampling.

use spade::{ConstrainedDelaunayTriangulation, HasPosition, Point2, Triangulation};
use std::collections::HashMap;

/// A terrain surface.
#[derive(Debug, Clone)]
pub struct Tin {
    /// The points: east, north, height (m).
    pub points: Vec<[f64; 3]>,
    /// The triangles, counter-clockwise seen from above, by the index of their points.
    pub triangles: Vec<[u32; 3]>,
    index: Buckets,
}

/// What a surface is built from.
#[derive(Debug, Clone, Default)]
pub struct Input<'a> {
    pub points: &'a [[f64; 3]],
    /// Lines the triangles follow; their vertices are points of the surface too.
    pub breaklines: &'a [Vec<[f64; 3]>],
    /// The outline the surface stays within: a closed ring (the last point joins the first).
    pub boundary: Option<&'a [[f64; 2]]>,
    /// Triangles on the outside with an outer edge longer than this are taken away (m).
    pub max_edge: Option<f64>,
}

/// A surface as built, with what was left out.
#[derive(Debug, Clone)]
pub struct Built {
    pub tin: Tin,
    /// Points left out: within a millimetre of an earlier one.
    pub duplicates: usize,
    /// Pieces of breaklines left out: they cross a breakline laid before.
    pub crossing: usize,
}

/// Volumes between a surface and a level or another surface (m³).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Volume {
    /// Where the surface lies above (cut, when the other is the design).
    pub above: f64,
    /// Where the surface lies below (fill).
    pub below: f64,
}

#[derive(Clone, Copy)]
struct Vertex {
    p: [f64; 3],
}

impl HasPosition for Vertex {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        Point2::new(self.p[0], self.p[1])
    }
}

/// A point's place to the millimetre, to find points that coincide.
fn key(x: f64, y: f64) -> (i64, i64) {
    ((x * 1000.0).round() as i64, (y * 1000.0).round() as i64)
}

impl Tin {
    /// The surface of the points, breaklines and boundary of `input`.
    pub fn build(input: &Input) -> Result<Built, String> {
        let mut cdt = ConstrainedDelaunayTriangulation::<Vertex>::new();
        let mut placed = HashMap::new();
        let mut duplicates = 0;
        let mut crossing = 0;
        let mut insert = |cdt: &mut ConstrainedDelaunayTriangulation<Vertex>, p: [f64; 3], count: bool, duplicates: &mut usize| {
            if !p.iter().all(|v| v.is_finite()) {
                return Ok(None);
            }
            if let Some(handle) = placed.get(&key(p[0], p[1])) {
                *duplicates += usize::from(count);
                return Ok(Some(*handle));
            }
            let handle = cdt.insert(Vertex { p }).map_err(|e| format!("a point cannot be placed in the surface: {e:?}"))?;
            placed.insert(key(p[0], p[1]), handle);
            Ok::<_, String>(Some(handle))
        };
        for p in input.points {
            insert(&mut cdt, *p, true, &mut duplicates)?;
        }
        for line in input.breaklines {
            let mut before = None;
            for p in line {
                let Some(handle) = insert(&mut cdt, *p, false, &mut duplicates)? else { continue };
                if let Some(from) = before.filter(|from| *from != handle) {
                    if cdt.can_add_constraint(from, handle) {
                        cdt.add_constraint(from, handle);
                    } else {
                        crossing += 1;
                    }
                }
                before = Some(handle);
            }
        }
        if let Some(ring) = input.boundary.filter(|ring| ring.len() >= 3) {
            // the corners of the outline get the height the surface has there, so the surface
            // ends on the outline itself
            let open = collect(&cdt);
            let nearest = |x: f64, y: f64| open.points.iter().min_by(|a, b| distance2(a, x, y).total_cmp(&distance2(b, x, y))).map(|p| p[2]);
            let mut handles = Vec::new();
            for [x, y] in ring {
                let Some(z) = open.height(*x, *y).or_else(|| nearest(*x, *y)) else { continue };
                if let Some(handle) = insert(&mut cdt, [*x, *y, z], false, &mut duplicates)? {
                    handles.push(handle);
                }
            }
            for k in 0..handles.len() {
                let (from, to) = (handles[k], handles[(k + 1) % handles.len()]);
                if from != to && cdt.can_add_constraint(from, to) {
                    cdt.add_constraint(from, to);
                }
            }
        }
        let mut tin = collect(&cdt);
        if let Some(ring) = input.boundary.filter(|ring| ring.len() >= 3) {
            let points = &tin.points;
            let inside = |t: &[u32; 3]| {
                let c = t.iter().fold([0.0, 0.0], |c, i| [c[0] + points[*i as usize][0] / 3.0, c[1] + points[*i as usize][1] / 3.0]);
                in_ring(ring, c[0], c[1])
            };
            let triangles = tin.triangles.iter().copied().filter(inside).collect();
            tin = Tin::from_parts(tin.points, triangles);
        }
        if let Some(max_edge) = input.max_edge.filter(|m| *m > 0.0) {
            tin = tin.peeled(max_edge);
        }
        if tin.triangles.is_empty() {
            return Err("a surface needs three points that are not on one line".into());
        }
        Ok(Built { tin, duplicates, crossing })
    }

    /// The surface of points and triangles as given.
    pub fn from_parts(points: Vec<[f64; 3]>, triangles: Vec<[u32; 3]>) -> Self {
        let index = Buckets::new(&points, &triangles);
        Tin { points, triangles, index }
    }

    /// The surface of a grid of `cols` by `rows` heights, row after row: the point of column
    /// `c` and row `r` lies at `at(c, r)`. A height that is not a number is a gap: the cells
    /// around it are left open.
    pub fn from_grid(cols: usize, rows: usize, heights: &[f32], at: impl Fn(usize, usize) -> [f64; 2]) -> Self {
        let mut ids = vec![u32::MAX; cols * rows];
        let mut points = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                let Some(z) = heights.get(r * cols + c).copied().filter(|z| z.is_finite()) else { continue };
                let [x, y] = at(c, r);
                ids[r * cols + c] = points.len() as u32;
                points.push([x, y, f64::from(z)]);
            }
        }
        let mut triangles = Vec::new();
        for r in 0..rows.saturating_sub(1) {
            for c in 0..cols.saturating_sub(1) {
                let quad = [ids[r * cols + c], ids[r * cols + c + 1], ids[(r + 1) * cols + c + 1], ids[(r + 1) * cols + c]];
                if quad.contains(&u32::MAX) {
                    continue;
                }
                for t in [[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]] {
                    // counter-clockwise from above, whichever way the rows run
                    let turn = cross(&points[t[0] as usize], &points[t[1] as usize], &points[t[2] as usize]);
                    triangles.push(if turn < 0.0 { [t[0], t[2], t[1]] } else { t });
                }
            }
        }
        Tin::from_parts(points, triangles)
    }

    /// The surface without the triangles on its outside that have an outer edge longer than
    /// `max_edge`: taken away from the outside in, until every outer edge is short enough.
    fn peeled(self, max_edge: f64) -> Self {
        let mut alive = vec![true; self.triangles.len()];
        let mut users: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for (t, triangle) in self.triangles.iter().enumerate() {
            for edge in edges(triangle) {
                users.entry(edge).or_default().push(t);
            }
        }
        let length = |(a, b): (u32, u32)| distance2(&self.points[a as usize], self.points[b as usize][0], self.points[b as usize][1]).sqrt();
        let mut queue: Vec<usize> = (0..self.triangles.len()).collect();
        while let Some(t) = queue.pop() {
            if !alive[t] {
                continue;
            }
            let outer_and_long = edges(&self.triangles[t]).into_iter().any(|edge| users[&edge].iter().filter(|u| alive[**u]).count() == 1 && length(edge) > max_edge);
            if outer_and_long {
                alive[t] = false;
                // its neighbours now lie on the outside
                for edge in edges(&self.triangles[t]) {
                    queue.extend(users[&edge].iter().copied().filter(|u| alive[*u]));
                }
            }
        }
        let triangles = self.triangles.iter().zip(&alive).filter(|(_, alive)| **alive).map(|(t, _)| *t).collect();
        Tin::from_parts(self.points, triangles)
    }

    /// The height of the surface at a place; `None` outside it.
    pub fn height(&self, x: f64, y: f64) -> Option<f64> {
        self.index.around(x, y)?.iter().find_map(|t| {
            let [a, b, c] = self.corners(*t as usize);
            let area = cross(&a, &b, &c);
            if area.abs() < 1e-12 {
                return None;
            }
            let p = [x, y, 0.0];
            let (wa, wb, wc) = (cross(&p, &b, &c) / area, cross(&a, &p, &c) / area, cross(&a, &b, &p) / area);
            (wa >= -1e-9 && wb >= -1e-9 && wc >= -1e-9).then(|| wa * a[2] + wb * b[2] + wc * c[2])
        })
    }

    /// The lowest and the highest height of the surface.
    pub fn range(&self) -> Option<(f64, f64)> {
        let mut used = self.triangles.iter().flatten().map(|i| self.points[*i as usize][2]);
        let first = used.next()?;
        Some(used.fold((first, first), |(low, high), z| (low.min(z), high.max(z))))
    }

    /// The area of the surface seen from above (m²).
    pub fn area(&self) -> f64 {
        (0..self.triangles.len()).map(|t| self.plan_area(t)).sum()
    }

    fn corners(&self, t: usize) -> [[f64; 3]; 3] {
        self.triangles[t].map(|i| self.points[i as usize])
    }

    fn plan_area(&self, t: usize) -> f64 {
        let [a, b, c] = self.corners(t);
        cross(&a, &b, &c).abs() / 2.0
    }

    /// The contour of a level: its lines over the surface, each a chain of places. A contour
    /// that closes on itself ends on its first place.
    pub fn contour(&self, level: f64) -> Vec<Vec<[f64; 2]>> {
        // a point on the level counts as below it, so a line never splits at a point
        let above = |i: u32| self.points[i as usize][2] > level;
        let mut places: HashMap<(u32, u32), [f64; 2]> = HashMap::new();
        let mut links: HashMap<(u32, u32), Vec<(u32, u32)>> = HashMap::new();
        for triangle in &self.triangles {
            let crossed: Vec<(u32, u32)> = edges(triangle).into_iter().filter(|(a, b)| above(*a) != above(*b)).collect();
            let [from, to] = crossed[..] else { continue };
            for (a, b) in [from, to] {
                places.entry((a, b)).or_insert_with(|| {
                    let (p, q) = (self.points[a as usize], self.points[b as usize]);
                    let t = (level - p[2]) / (q[2] - p[2]);
                    [p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]
                });
            }
            links.entry(from).or_default().push(to);
            links.entry(to).or_default().push(from);
        }
        // the open lines first, from their ends; what is left closes on itself
        let mut starts: Vec<(u32, u32)> = links.iter().filter(|(_, to)| to.len() == 1).map(|(from, _)| *from).collect();
        starts.sort_unstable();
        let mut rest: Vec<(u32, u32)> = links.keys().copied().collect();
        rest.sort_unstable();
        let mut done: HashMap<(u32, u32), bool> = HashMap::new();
        let mut lines = Vec::new();
        for start in starts.into_iter().chain(rest) {
            if done.contains_key(&start) {
                continue;
            }
            let mut line = vec![places[&start]];
            let mut at = start;
            done.insert(at, true);
            while let Some(next) = links[&at].iter().copied().find(|next| !done.contains_key(next)) {
                line.push(places[&next]);
                done.insert(next, true);
                at = next;
            }
            // back at the start: a closed contour
            if line.len() > 2 && links[&at].contains(&start) && links[&start].len() == 2 {
                line.push(places[&start]);
            }
            if line.len() >= 2 {
                lines.push(line);
            }
        }
        lines
    }

    /// The contours every `interval` metres of height, lowest first: each its level and lines.
    pub fn contours(&self, interval: f64) -> Vec<(f64, Vec<Vec<[f64; 2]>>)> {
        let Some((low, high)) = self.range().filter(|_| interval > 0.0) else { return Vec::new() };
        let (first, last) = ((low / interval).ceil() as i64, (high / interval).floor() as i64);
        (first..=last)
            .map(|k| k as f64 * interval)
            .filter(|level| *level < high)
            .map(|level| (level, self.contour(level)))
            .filter(|(_, lines)| !lines.is_empty())
            .collect()
    }

    /// The volumes between the surface and a level: exact, triangle by triangle.
    pub fn volume(&self, level: f64) -> Volume {
        let mut volume = Volume::default();
        for t in 0..self.triangles.len() {
            let area = self.plan_area(t);
            let h = self.corners(t).map(|p| p[2] - level);
            let net = area * (h[0] + h[1] + h[2]) / 3.0;
            let over = h.iter().filter(|h| **h > 0.0).count();
            match over {
                3 => volume.above += net,
                0 => volume.below -= net,
                _ => {
                    // the one corner on its own side of the level, with the two it leaves
                    let lone = if over == 1 { h.iter().position(|h| *h > 0.0) } else { h.iter().position(|h| *h <= 0.0) }.unwrap_or(0);
                    let (apex, left, right) = (h[lone], h[(lone + 1) % 3], h[(lone + 2) % 3]);
                    // the part of the triangle on the apex's side, and the wedge over it
                    let part = area * (apex / (apex - left)) * (apex / (apex - right));
                    let wedge = part * apex.abs() / 3.0;
                    if over == 1 {
                        volume.above += wedge;
                        volume.below += wedge - net;
                    } else {
                        volume.below += wedge;
                        volume.above += wedge + net;
                    }
                }
            }
        }
        volume
    }

    /// The volumes between this surface and `other` where both lie, sampled every `cell`
    /// metres: `above` where this one is the higher.
    pub fn volume_between(&self, other: &Tin, cell: f64) -> Volume {
        let mut volume = Volume::default();
        let (Some(mine), Some(theirs)) = (bounds(&self.points), bounds(&other.points)) else { return volume };
        let low = [mine[0].max(theirs[0]), mine[1].max(theirs[1])];
        let high = [mine[2].min(theirs[2]), mine[3].min(theirs[3])];
        if cell <= 0.0 || low[0] >= high[0] || low[1] >= high[1] {
            return volume;
        }
        let (cols, rows) = (((high[0] - low[0]) / cell).ceil() as usize, ((high[1] - low[1]) / cell).ceil() as usize);
        for r in 0..rows {
            for c in 0..cols {
                let (x, y) = (low[0] + (c as f64 + 0.5) * cell, low[1] + (r as f64 + 0.5) * cell);
                let (Some(a), Some(b)) = (self.height(x, y), other.height(x, y)) else { continue };
                if a > b {
                    volume.above += (a - b) * cell * cell;
                } else {
                    volume.below += (b - a) * cell * cell;
                }
            }
        }
        volume
    }
}

/// Fills the gaps of a grid of heights (values that are not a number) with the height of the
/// nearest cell that has one, working outwards from the cells that do. A grid without any
/// height stays as it is.
pub fn fill_gaps(cols: usize, rows: usize, heights: &mut [f32]) {
    let mut front: Vec<usize> = (0..cols * rows).filter(|i| heights.get(*i).is_some_and(|z| z.is_finite())).collect();
    while !front.is_empty() {
        let mut next = Vec::new();
        for i in front {
            let (c, r) = (i % cols, i / cols);
            let around = [(c > 0).then(|| i - 1), (c + 1 < cols).then(|| i + 1), (r > 0).then(|| i - cols), (r + 1 < rows).then(|| i + cols)];
            for j in around.into_iter().flatten() {
                if !heights[j].is_finite() {
                    heights[j] = heights[i];
                    next.push(j);
                }
            }
        }
        front = next;
    }
}

/// The points and triangles of a triangulation.
fn collect(cdt: &ConstrainedDelaunayTriangulation<Vertex>) -> Tin {
    let points = cdt.vertices().map(|v| v.data().p).collect();
    let triangles = cdt.inner_faces().map(|face| face.vertices().map(|v| v.fix().index() as u32)).collect();
    Tin::from_parts(points, triangles)
}

/// The three edges of a triangle, each named by its lower point first.
fn edges(t: &[u32; 3]) -> [(u32, u32); 3] {
    [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])].map(|(a, b)| (a.min(b), a.max(b)))
}

/// Twice the area of a triangle seen from above; positive when counter-clockwise.
fn cross(a: &[f64; 3], b: &[f64; 3], c: &[f64; 3]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn distance2(p: &[f64; 3], x: f64, y: f64) -> f64 {
    (p[0] - x).powi(2) + (p[1] - y).powi(2)
}

/// Whether a place lies within a closed ring.
fn in_ring(ring: &[[f64; 2]], x: f64, y: f64) -> bool {
    let mut inside = false;
    for k in 0..ring.len() {
        let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
        if (a[1] > y) != (b[1] > y) && x < a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            inside = !inside;
        }
    }
    inside
}

/// The lowest and highest east and north of points: west, south, east, north.
fn bounds(points: &[[f64; 3]]) -> Option<[f64; 4]> {
    let first = points.first()?;
    Some(points.iter().fold([first[0], first[1], first[0], first[1]], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]))
}

/// The triangles of a surface by where they lie, to find the one over a place.
#[derive(Debug, Clone, Default)]
struct Buckets {
    west: f64,
    south: f64,
    cell: f64,
    cols: usize,
    rows: usize,
    triangles: Vec<Vec<u32>>,
}

impl Buckets {
    fn new(points: &[[f64; 3]], triangles: &[[u32; 3]]) -> Self {
        let Some([west, south, east, north]) = bounds(points).filter(|_| !triangles.is_empty()) else { return Buckets::default() };
        // about two triangles a bucket
        let side = ((triangles.len() as f64 / 2.0).sqrt().ceil() as usize).clamp(1, 2048);
        let cell = ((east - west).max(north - south) / side as f64).max(1e-9);
        let (cols, rows) = ((((east - west) / cell).floor() as usize + 1).min(4096), (((north - south) / cell).floor() as usize + 1).min(4096));
        let mut buckets = Buckets { west, south, cell, cols, rows, triangles: vec![Vec::new(); cols * rows] };
        for (t, triangle) in triangles.iter().enumerate() {
            let corners = triangle.map(|i| points[i as usize]);
            let column = |x: f64| (((x - west) / cell).floor() as usize).min(cols - 1);
            let row = |y: f64| (((y - south) / cell).floor() as usize).min(rows - 1);
            let (c0, c1) = (column(corners.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min)), column(corners.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max)));
            let (r0, r1) = (row(corners.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min)), row(corners.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max)));
            for r in r0..=r1 {
                for c in c0..=c1 {
                    buckets.triangles[r * cols + c].push(t as u32);
                }
            }
        }
        buckets
    }

    /// The triangles that may lie over a place.
    fn around(&self, x: f64, y: f64) -> Option<&Vec<u32>> {
        if self.triangles.is_empty() || x < self.west || y < self.south {
            return None;
        }
        let (c, r) = (((x - self.west) / self.cell).floor() as usize, ((y - self.south) / self.cell).floor() as usize);
        (c < self.cols && r < self.rows).then(|| &self.triangles[r * self.cols + c])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 40 by 40 m square of points every 10 m on a slope: one metre up every ten to the east.
    fn ramp() -> Vec<[f64; 3]> {
        (0..5).flat_map(|r| (0..5).map(move |c| [c as f64 * 10.0, r as f64 * 10.0, c as f64])).collect()
    }

    fn built(input: &Input) -> Tin {
        Tin::build(input).unwrap().tin
    }

    #[test]
    fn points_on_a_slope_make_a_surface_with_the_height_of_the_slope_everywhere() {
        let tin = built(&Input { points: &ramp(), ..Input::default() });
        assert_eq!((tin.points.len(), tin.triangles.len()), (25, 32));
        assert!((tin.area() - 1600.0).abs() < 1e-9);
        assert_eq!(tin.range(), Some((0.0, 4.0)));
        for (x, y) in [(12.5, 31.0), (0.0, 0.0), (40.0, 40.0), (39.9, 0.1), (20.0, 20.0)] {
            assert!((tin.height(x, y).unwrap() - x / 10.0).abs() < 1e-9, "{x} {y}");
        }
        // outside the surface there is no height
        assert_eq!(tin.height(-0.5, 20.0), None);
        assert_eq!(tin.height(20.0, 40.5), None);
    }

    #[test]
    fn points_within_a_millimetre_count_once_and_three_on_a_line_make_no_surface() {
        let mut points = ramp();
        points.push([10.0002, 10.0, 9.0]);
        let built = Tin::build(&Input { points: &points, ..Input::default() }).unwrap();
        assert_eq!((built.duplicates, built.tin.points.len()), (1, 25));
        assert!((built.tin.height(10.0, 10.0).unwrap() - 1.0).abs() < 1e-9);
        assert!(Tin::build(&Input { points: &[[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 2.0, 0.0]], ..Input::default() }).is_err());
        assert!(Tin::build(&Input::default()).is_err());
    }

    #[test]
    fn a_breakline_is_an_edge_of_the_surface() {
        // four corners of a square, a ditch across it from west to east that no triangle of the
        // plain triangulation follows
        let points = [[0.0, 0.0, 1.0], [20.0, 0.0, 1.0], [20.0, 10.0, 1.0], [0.0, 10.0, 1.0], [10.0, 0.0, 1.0], [10.0, 10.0, 1.0]];
        let plain = built(&Input { points: &points, ..Input::default() });
        assert!((plain.height(10.0, 5.0).unwrap() - 1.0).abs() < 1e-9);
        let ditch = vec![vec![[2.0, 5.0, 0.0], [18.0, 5.0, 0.0]]];
        let made = Tin::build(&Input { points: &points, breaklines: &ditch, ..Input::default() }).unwrap();
        assert_eq!(made.crossing, 0);
        let tin = made.tin;
        // the bottom of the ditch is at its height all along, and the ends are points
        assert_eq!(tin.points.len(), 8);
        for x in [2.0, 6.0, 10.0, 14.0, 18.0] {
            assert!(tin.height(x, 5.0).unwrap().abs() < 1e-9, "{x}");
        }
        let is_edge = |a: [f64; 2], b: [f64; 2]| {
            let at = |p: [f64; 2]| tin.points.iter().position(|q| q[0] == p[0] && q[1] == p[1]).unwrap() as u32;
            let (a, b) = (at(a), at(b));
            tin.triangles.iter().any(|t| edges(t).contains(&(a.min(b), a.max(b))))
        };
        assert!(is_edge([2.0, 5.0], [18.0, 5.0]));
        // a breakline that crosses one laid before is left out where it crosses
        let crossed = vec![vec![[2.0, 5.0, 0.0], [18.0, 5.0, 0.0]], vec![[10.0, 1.0, 0.5], [10.0, 9.0, 0.5]]];
        assert_eq!(Tin::build(&Input { points: &points, breaklines: &crossed, ..Input::default() }).unwrap().crossing, 1);
    }

    #[test]
    fn a_boundary_keeps_the_surface_within_its_outline() {
        // an outline over the south-west of the ramp, with corners between the points
        let outline = [[5.0, 5.0], [25.0, 5.0], [25.0, 25.0], [5.0, 25.0]];
        let tin = built(&Input { points: &ramp(), boundary: Some(&outline), ..Input::default() });
        assert!((tin.area() - 400.0).abs() < 1e-6, "{}", tin.area());
        // the corners of the outline lie on the slope
        assert!((tin.height(5.0, 5.0).unwrap() - 0.5).abs() < 1e-9);
        assert!((tin.height(24.9, 24.9).unwrap() - 2.49).abs() < 1e-9);
        assert_eq!(tin.height(30.0, 30.0), None);
        assert_eq!(tin.height(2.0, 15.0), None);
    }

    #[test]
    fn long_triangles_across_an_open_side_are_taken_away() {
        // an L of points every 10 m: the triangulation closes the inside corner with long triangles
        let points: Vec<[f64; 3]> = ramp().into_iter().filter(|p| p[0] <= 10.0 || p[1] <= 10.0).collect();
        let closed = built(&Input { points: &points, ..Input::default() });
        assert!(closed.height(22.0, 22.0).is_some());
        let open = built(&Input { points: &points, max_edge: Some(12.0), ..Input::default() });
        assert_eq!(open.height(22.0, 22.0), None);
        assert!((open.area() - 700.0).abs() < 1e-9, "{}", open.area());
        assert!((open.height(5.0, 35.0).unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_contour_runs_where_the_surface_has_its_level() {
        let tin = built(&Input { points: &ramp(), ..Input::default() });
        let lines = tin.contour(2.5);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert!(line.iter().all(|p| (p[0] - 25.0).abs() < 1e-9), "{line:?}");
        let (south, north) = (line.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min), line.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max));
        assert_eq!((south, north), (0.0, 40.0));
        // every metre: the levels the slope passes, without its own top
        let levels: Vec<f64> = tin.contours(1.0).into_iter().map(|(level, _)| level).collect();
        assert_eq!(levels, vec![0.0, 1.0, 2.0, 3.0]);
        assert!(tin.contour(7.0).is_empty());
        assert!(tin.contours(0.0).is_empty());
    }

    #[test]
    fn a_hill_has_contours_that_close_on_themselves() {
        // a cone: 2 m high in the middle of a 40 m square
        let points: Vec<[f64; 3]> = (0..=8).flat_map(|r| (0..=8).map(move |c| (c as f64 * 5.0, r as f64 * 5.0))).map(|(x, y)| [x, y, (2.0 - ((x - 20.0).hypot(y - 20.0)) / 10.0).max(0.0)]).collect();
        let tin = built(&Input { points: &points, ..Input::default() });
        let lines = tin.contour(1.0);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line.first(), line.last());
        assert!(line.len() > 8);
        // all of it about 10 m from the top
        assert!(line.iter().all(|p| ((p[0] - 20.0).hypot(p[1] - 20.0) - 10.0).abs() < 1.6), "{line:?}");
    }

    #[test]
    fn the_volume_against_a_level_is_exact() {
        let tin = built(&Input { points: &ramp(), ..Input::default() });
        // the slope from 0 to 4 m over 40 by 40 m, against 2 m: a wedge of 800 m³ each way
        let half = tin.volume(2.0);
        assert!((half.above - 800.0).abs() < 1e-9 && (half.below - 800.0).abs() < 1e-9, "{half:?}");
        let all = tin.volume(0.0);
        assert!((all.above - 3200.0).abs() < 1e-9 && all.below.abs() < 1e-9, "{all:?}");
        // a level between the points, 0.5 m: below it a wedge 5 m wide and 0.5 m deep along 40 m
        let low = tin.volume(0.5);
        assert!((low.below - 50.0).abs() < 1e-9 && (low.above - 2450.0).abs() < 1e-9, "{low:?}");
    }

    #[test]
    fn the_volume_between_two_surfaces_is_sampled_where_both_lie() {
        let ground = built(&Input { points: &ramp(), ..Input::default() });
        // a flat design at 2 m over the west half and beyond the ground to the west
        let design = Tin::from_parts(vec![[-10.0, 0.0, 2.0], [20.0, 0.0, 2.0], [20.0, 40.0, 2.0], [-10.0, 40.0, 2.0]], vec![[0, 1, 2], [0, 2, 3]]);
        let volume = design.volume_between(&ground, 0.5);
        // over 20 by 40 m the ground runs from 0 to 2 m: the design lies 1 m above it on average
        assert!((volume.above - 800.0).abs() < 1.0 && volume.below.abs() < 1e-9, "{volume:?}");
        let other_way = ground.volume_between(&design, 0.5);
        assert!((other_way.below - 800.0).abs() < 1.0 && other_way.above.abs() < 1e-9);
        assert_eq!(ground.volume_between(&design, 0.0), Volume::default());
    }

    #[test]
    fn a_grid_of_heights_is_a_surface_with_its_gaps_open_or_filled() {
        // 4 by 3 heights, rows from north to south, one metre apart; one height missing
        let mut heights = vec![1.0, 1.0, 1.0, 1.0, 2.0, f32::NAN, 2.0, 2.0, 3.0, 3.0, 3.0, 3.0];
        let at = |c: usize, r: usize| [100.0 + c as f64, 50.0 - r as f64];
        let open = Tin::from_grid(4, 3, &heights, at);
        // the four cells around the gap are open: two of six left
        assert_eq!((open.points.len(), open.triangles.len()), (11, 4));
        assert!(open.triangles.iter().all(|t| cross(&open.points[t[0] as usize], &open.points[t[1] as usize], &open.points[t[2] as usize]) > 0.0));
        assert_eq!(open.height(100.5, 49.5), None);
        assert!((open.height(102.5, 49.5).unwrap() - 1.5).abs() < 1e-9);
        fill_gaps(4, 3, &mut heights);
        assert!(heights[5].is_finite());
        let filled = Tin::from_grid(4, 3, &heights, at);
        assert_eq!(filled.triangles.len(), 12);
        assert!((filled.area() - 6.0).abs() < 1e-9);
        assert!(filled.height(100.5, 49.5).is_some());
        // nothing to fill from
        let mut none = vec![f32::NAN; 4];
        fill_gaps(2, 2, &mut none);
        assert!(none.iter().all(|z| z.is_nan()));
    }
}
