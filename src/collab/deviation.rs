//! How far the points of a point cloud lie from the surfaces of a model: what was built against
//! what was drawn.
//!
//! The model's triangles go into a tree of boxes, split in the middle of their longest side
//! until a box holds a few; the distance of a point is looked up by going down the boxes nearest
//! to it and leaving out those further off than what was found (or than `reach`: a point that
//! far from any surface has nothing drawn near it). The points are looked up on all the
//! processor's cores.

use super::clash::{nearest_on, Triangle};
use glam::Vec3;

/// The triangles a leaf holds at the most.
const LEAF: usize = 8;

struct Node {
    lo: Vec3,
    hi: Vec3,
    /// A leaf: its triangles from `first`, `count` of them. Else `count` is 0 and its two halves
    /// are the node after it and node `first`.
    first: u32,
    count: u32,
}

/// The surfaces of a model, to measure distances to.
pub struct Surface {
    triangles: Vec<Triangle>,
    nodes: Vec<Node>,
}

fn bounds(triangles: &[Triangle]) -> (Vec3, Vec3) {
    triangles.iter().flatten().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)))
}

impl Surface {
    pub fn new(mut triangles: Vec<Triangle>) -> Surface {
        let mut nodes = Vec::with_capacity(triangles.len() / 4 + 1);
        if !triangles.is_empty() {
            let count = triangles.len();
            build(&mut triangles, 0, count, &mut nodes);
        }
        Surface { triangles, nodes }
    }

    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    /// How far `p` is from the nearest surface, when that is within `reach`.
    pub fn distance(&self, p: Vec3, reach: f32) -> Option<f32> {
        self.nearest(p, reach).map(|q| q.distance(p))
    }

    /// The place on the surfaces nearest to `p`, when that is within `reach`.
    pub fn nearest(&self, p: Vec3, reach: f32) -> Option<Vec3> {
        let mut best = reach * reach;
        let mut found: Option<Vec3> = None;
        let mut stack: Vec<usize> = Vec::with_capacity(64);
        if !self.nodes.is_empty() {
            stack.push(0);
        }
        // the square of how far `p` is from a box
        let off = |n: &Node| (n.lo - p).max(p - n.hi).max(Vec3::ZERO).length_squared();
        while let Some(k) = stack.pop() {
            let node = &self.nodes[k];
            if off(node) > best {
                continue;
            }
            if node.count > 0 {
                for t in &self.triangles[node.first as usize..(node.first + node.count) as usize] {
                    let q = nearest_on(p, t);
                    let d = q.distance_squared(p);
                    if d <= best {
                        best = d;
                        found = Some(q);
                    }
                }
                continue;
            }
            // the nearer half last, so it is looked at first
            let (a, b) = (k + 1, node.first as usize);
            if off(&self.nodes[a]) <= off(&self.nodes[b]) {
                stack.push(b);
                stack.push(a);
            } else {
                stack.push(a);
                stack.push(b);
            }
        }
        found
    }

    /// The distance of each point, on all cores; `None` where no surface is within `reach`.
    pub fn distances(&self, points: &[Vec3], reach: f32) -> Vec<Option<f32>> {
        let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 16);
        let chunk = points.len().div_ceil(workers).max(1);
        let mut out = vec![None; points.len()];
        std::thread::scope(|scope| {
            for (from, to) in points.chunks(chunk).zip(out.chunks_mut(chunk)) {
                scope.spawn(move || {
                    for (p, d) in from.iter().zip(to.iter_mut()) {
                        *d = self.distance(*p, reach);
                    }
                });
            }
        });
        out
    }
}

/// Builds the node of triangles `from..to` (and its halves after it); returns its place.
fn build(triangles: &mut [Triangle], from: usize, to: usize, nodes: &mut Vec<Node>) -> usize {
    let (lo, hi) = bounds(&triangles[from..to]);
    let at = nodes.len();
    nodes.push(Node { lo, hi, first: from as u32, count: (to - from) as u32 });
    if to - from <= LEAF {
        return at;
    }
    // split where the middles of the triangles fall on the longest side
    let size = hi - lo;
    let axis = if size.x >= size.y && size.x >= size.z { 0 } else if size.y >= size.z { 1 } else { 2 };
    let middle = |t: &Triangle| t[0][axis] + t[1][axis] + t[2][axis];
    let half = (to - from) / 2;
    triangles[from..to].select_nth_unstable_by(half, |x, y| middle(x).total_cmp(&middle(y)));
    build(triangles, from, from + half, nodes);
    let right = build(triangles, from + half, to, nodes);
    nodes[at].first = right as u32;
    nodes[at].count = 0;
    at
}

/// How a scan is to be moved to lie on the model: the points are moved, time and again, by the
/// mean of what brings each to the nearest surface, looking nearer by each time (from `reach`
/// down to a tenth of it), until they lie still. Only a move, no turn: a scan that is turned is
/// to be laid right first. Gives the move and how many points had a surface near at the end.
pub fn fit_shift(surface: &Surface, points: &[Vec3], reach: f32) -> (Vec3, usize) {
    let mut shift = Vec3::ZERO;
    let mut near = 0;
    let mut within = reach;
    for _ in 0..40 {
        let (mut sum, mut count) = (Vec3::ZERO, 0usize);
        for p in points {
            let moved = *p + shift;
            if let Some(q) = surface.nearest(moved, within) {
                sum += q - moved;
                count += 1;
            }
        }
        near = count;
        if count == 0 {
            break;
        }
        let step = sum / count as f32;
        shift += step;
        within = (within * 0.7).max(reach * 0.1);
        if step.length() < 0.0002 {
            break;
        }
    }
    (shift, near)
}

/// A place where many points lie further from the model than allowed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HotSpot {
    /// The middle of its points.
    pub at: Vec3,
    pub points: usize,
    /// The furthest of them (metres).
    pub worst: f32,
}

/// What the comparison of a cloud with a model gave.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub points: usize,
    /// Within the tolerance; further than it but near a surface; with no surface within reach.
    pub within: usize,
    pub off: usize,
    pub far: usize,
    /// Of the points near a surface: the mean distance and the largest (metres).
    pub mean: f32,
    pub worst: f32,
    /// How many points lie in each fifth of the tolerance, up to twice the tolerance, and beyond.
    pub histogram: [usize; 11],
    pub hot_spots: Vec<HotSpot>,
}

/// The size of the cells the points off are gathered in (metres).
const CELL: f32 = 1.0;

/// Sums the distances up: how many points are within `tolerance`, the mean and the worst, and
/// the places where the most points are off, the fullest first (at most `most`).
pub fn summarise(points: &[Vec3], distances: &[Option<f32>], tolerance: f32, most: usize) -> Summary {
    let mut s = Summary { points: points.len(), ..Summary::default() };
    let mut sum = 0.0f64;
    let mut cells: std::collections::HashMap<[i32; 3], (Vec3, usize, f32)> = std::collections::HashMap::new();
    for (p, d) in points.iter().zip(distances) {
        let Some(d) = *d else {
            s.far += 1;
            continue;
        };
        sum += f64::from(d);
        s.worst = s.worst.max(d);
        s.histogram[((d / tolerance.max(1e-6) * 5.0) as usize).min(10)] += 1;
        if d <= tolerance {
            s.within += 1;
        } else {
            s.off += 1;
            let cell = cells.entry((*p / CELL).floor().to_array().map(|v| v as i32)).or_insert((Vec3::ZERO, 0, 0.0));
            cell.0 += *p;
            cell.1 += 1;
            cell.2 = cell.2.max(d);
        }
    }
    let near = s.within + s.off;
    s.mean = if near > 0 { (sum / near as f64) as f32 } else { 0.0 };
    // a few stray points are no hot spot
    let mut spots: Vec<HotSpot> = cells.into_values().filter(|c| c.1 >= 20).map(|(sum, n, worst)| HotSpot { at: sum / n as f32, points: n, worst }).collect();
    spots.sort_by(|a, b| b.points.cmp(&a.points).then(b.worst.total_cmp(&a.worst)));
    spots.truncate(most);
    s.hot_spots = spots;
    s
}

/// The colour of a point by how far it is off: green within the tolerance, through yellow at it
/// to red at twice it and beyond; grey with no surface near.
pub fn colour(distance: Option<f32>, tolerance: f32) -> [u8; 4] {
    let Some(d) = distance else { return [150, 150, 150, 255] };
    let t = (d / tolerance.max(1e-6)).clamp(0.0, 2.0);
    let (r, g) = if t <= 1.0 { (40.0 + 200.0 * t, 170.0 + 30.0 * t) } else { (240.0 - 20.0 * (t - 1.0), 200.0 - 160.0 * (t - 1.0)) };
    [r as u8, g as u8, 40, 255]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A floor of 10 × 10 m at z = 0 as many small triangles, and a wall on its edge.
    fn room() -> Surface {
        let mut triangles = Vec::new();
        for i in 0..20 {
            for j in 0..20 {
                let (x, y) = (i as f32 * 0.5, j as f32 * 0.5);
                let p = |dx: f32, dy: f32| Vec3::new(x + dx, y + dy, 0.0);
                triangles.push([p(0.0, 0.0), p(0.5, 0.0), p(0.5, 0.5)]);
                triangles.push([p(0.0, 0.0), p(0.5, 0.5), p(0.0, 0.5)]);
            }
        }
        triangles.push([Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 3.0)]);
        triangles.push([Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 3.0), Vec3::new(0.0, 0.0, 3.0)]);
        Surface::new(triangles)
    }

    #[test]
    fn a_point_is_as_far_as_the_nearest_surface_and_too_far_has_none() {
        let s = room();
        let d = |x: f32, y: f32, z: f32| s.distance(Vec3::new(x, y, z), 0.5);
        assert!((d(5.0, 5.0, 0.03).unwrap() - 0.03).abs() < 1e-5);
        // nearer the wall than the floor
        assert!((d(4.0, 0.02, 1.5).unwrap() - 0.02).abs() < 1e-5);
        // past the edge of the floor: to its edge
        assert!((d(10.3, 5.0, 0.0).unwrap() - 0.3).abs() < 1e-5);
        assert_eq!(d(5.0, 5.0, 1.5), None);
        // every point the same as looked up one by one, in their order
        let points: Vec<Vec3> = (0..500).map(|k| Vec3::new(k as f32 * 0.02, 5.0, (k % 7) as f32 * 0.05)).collect();
        let all = s.distances(&points, 0.5);
        assert!(points.iter().zip(&all).all(|(p, d)| s.distance(*p, 0.5) == *d));
        assert!(Surface::new(Vec::new()).distance(Vec3::ZERO, 1.0).is_none());
    }

    #[test]
    fn a_scan_laid_a_little_off_is_moved_onto_the_model() {
        let s = room();
        // the floor and the wall scanned, laid 30 mm up and 20 mm off the wall
        let off = Vec3::new(0.0, 0.02, 0.03);
        let mut points = Vec::new();
        for i in 0..60 {
            for j in 0..60 {
                let (u, v) = (i as f32 * 0.15 + 0.4, j as f32 * 0.15 + 0.4);
                points.push(Vec3::new(u, v, 0.0) + off);
                if j < 18 {
                    points.push(Vec3::new(u, 0.0, v) + off);
                }
            }
        }
        let (shift, near) = fit_shift(&s, &points, 0.5);
        assert!(near > points.len() * 9 / 10, "{near} of {}", points.len());
        // back onto the floor and the wall; along them nothing says where it belongs
        assert!((shift.z + 0.03).abs() < 0.002 && (shift.y + 0.02).abs() < 0.004 && shift.x.abs() < 0.004, "{shift:?}");
    }

    #[test]
    fn the_points_off_gather_in_hot_spots() {
        let s = room();
        // a floor scanned flat, but with a bump of 60 mm around (2.5, 2.5), and a chair in the air
        let mut points = Vec::new();
        for i in 0..100 {
            for j in 0..100 {
                let (x, y) = (i as f32 * 0.1 + 0.05, j as f32 * 0.1 + 0.05);
                let bump = if (x - 2.5).abs() < 0.5 && (y - 2.5).abs() < 0.5 { 0.06 } else { 0.004 };
                points.push(Vec3::new(x, y, bump));
            }
        }
        points.extend((0..50).map(|k| Vec3::new(7.0 + k as f32 * 0.01, 7.0, 1.6)));
        let distances = s.distances(&points, 0.5);
        let sum = summarise(&points, &distances, 0.02, 5);
        assert_eq!((sum.points, sum.far, sum.off), (10_050, 50, 100));
        assert!((sum.worst - 0.06).abs() < 1e-4 && sum.mean < 0.006, "{sum:?}");
        assert_eq!((sum.hot_spots.len(), sum.hot_spots[0].points), (1, 100), "the bump is one place: {:?}", sum.hot_spots);
        assert!(sum.hot_spots.iter().all(|h| (h.at.x - 2.5).abs() < 0.5 && (h.worst - 0.06).abs() < 1e-4));
        // green within, red at twice the tolerance, grey with nothing near
        assert!(colour(Some(0.0), 0.02)[1] > colour(Some(0.0), 0.02)[0]);
        assert!(colour(Some(0.05), 0.02)[0] > 200 && colour(Some(0.05), 0.02)[1] < 60);
        assert_eq!(colour(None, 0.02), [150, 150, 150, 255]);
    }
}
