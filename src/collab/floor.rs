//! The floor of a storey as a grid, to walk over: where the rooms are, where the doors lead from
//! one to the next, and where a way out is.
//!
//! Each room is laid on the grid by its own shape (its triangles seen from above), so the walk
//! goes round the corner of an L-shaped room and round a core, not through them. A door joins
//! the rooms it lies between; a door with a room on one side only leads outside, and a stair is
//! a way out of its storey. From the ways out the grid is filled with how far each cell is to
//! walk; a room's walk is that of its farthest cell, pulled straight where nothing is in the way.
//!
//! The same grid says how wide a room is at its widest (the largest circle that fits in it: a
//! wheelchair turns there), how much room there is at each side of a door, and which rooms hang
//! together through doors that do not hold back fire.

use glam::{Vec2, Vec3};
use std::collections::BinaryHeap;

/// A room: its box and its shape seen from above (triangles).
#[derive(Debug, Clone)]
pub struct Room {
    pub min: Vec3,
    pub max: Vec3,
    pub shape: Vec<[Vec2; 3]>,
}

impl Room {
    /// A room that is its box.
    pub fn of_box(min: Vec3, max: Vec3) -> Room {
        let (a, b, c, d) = (Vec2::new(min.x, min.y), Vec2::new(max.x, min.y), Vec2::new(max.x, max.y), Vec2::new(min.x, max.y));
        Room { min, max, shape: vec![[a, b, c], [a, c, d]] }
    }
}

/// A door or a stair: its box, and for a door whether it holds back fire.
#[derive(Debug, Clone, Copy)]
pub struct Way {
    pub min: Vec3,
    pub max: Vec3,
    pub fire: bool,
}

/// What the grid says of a storey's rooms and doors.
#[derive(Debug, Clone, Default)]
pub struct Walked {
    /// Per room: how far its farthest point is from a way out, and that walk; `None` when no
    /// way out is reached.
    pub escape: Vec<Option<(f32, Vec<Vec3>)>>,
    /// Per room: the largest circle that fits in it (its diameter, metres).
    pub widest: Vec<f32>,
    /// Per room: its floor (square metres) and how many doors it has.
    pub area: Vec<f32>,
    pub doors: Vec<usize>,
    /// Per door: the rooms it lies between (at most two) and the largest circle that fits in
    /// each within reach of the door.
    pub at_door: Vec<Vec<(usize, f32)>>,
    /// The rooms that hang together through doors that do not hold back fire, each group with
    /// its floor (square metres); only groups of more than one room or over no door at all.
    pub compartments: Vec<(Vec<usize>, f32)>,
}

const NONE: u32 = 0;
const DOOR: u32 = u32::MAX;

struct Grid {
    origin: Vec2,
    cell: f32,
    w: usize,
    h: usize,
    /// The room of each cell (its place + 1), [`DOOR`] in a doorway, [`NONE`] elsewhere.
    owner: Vec<u32>,
}

impl Grid {
    fn at(&self, p: Vec2) -> Option<usize> {
        let (x, y) = (((p.x - self.origin.x) / self.cell).floor(), ((p.y - self.origin.y) / self.cell).floor());
        (x >= 0.0 && y >= 0.0 && (x as usize) < self.w && (y as usize) < self.h).then(|| y as usize * self.w + x as usize)
    }

    fn centre(&self, k: usize) -> Vec2 {
        self.origin + Vec2::new((k % self.w) as f32 + 0.5, (k / self.w) as f32 + 0.5) * self.cell
    }

    /// The cells whose centre lies in the box from `min` to `max`.
    fn cells(&self, min: Vec2, max: Vec2) -> impl Iterator<Item = usize> + '_ {
        let lo = ((min - self.origin) / self.cell - 0.5).ceil().max(Vec2::ZERO);
        let hi = ((max - self.origin) / self.cell - 0.5).floor();
        let (x0, y0) = (lo.x as usize, lo.y as usize);
        let (x1, y1) = (if hi.x < 0.0 { 0 } else { (hi.x as usize + 1).min(self.w) }, if hi.y < 0.0 { 0 } else { (hi.y as usize + 1).min(self.h) });
        (y0..y1).flat_map(move |y| (x0..x1).map(move |x| y * self.w + x))
    }

    fn walkable(&self, k: usize) -> bool {
        self.owner[k] != NONE
    }

    /// Whether the straight line from `a` to `b` stays on the floor.
    fn clear(&self, a: Vec2, b: Vec2) -> bool {
        let steps = ((b - a).length() / (self.cell * 0.5)).ceil().max(1.0) as usize;
        (0..=steps).all(|s| self.at(a.lerp(b, s as f32 / steps as f32)).is_some_and(|k| self.walkable(k)))
    }
}

fn in_triangle(p: Vec2, t: &[Vec2; 3]) -> bool {
    let side = |a: Vec2, b: Vec2| (b - a).perp_dot(p - a);
    let (d0, d1, d2) = (side(t[0], t[1]), side(t[1], t[2]), side(t[2], t[0]));
    !((d0 < 0.0 || d1 < 0.0 || d2 < 0.0) && (d0 > 0.0 || d1 > 0.0 || d2 > 0.0))
}

#[derive(PartialEq)]
struct Step(f32, usize);
impl Eq for Step {}
impl Ord for Step {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.total_cmp(&self.0).then(self.1.cmp(&other.1))
    }
}
impl PartialOrd for Step {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// How far each cell is from the nearest of `from`, stepping over the cells `open` lets through
/// (straight and diagonal, never across a corner), and the cell each is reached from.
fn spread(grid: &Grid, from: &[usize], open: impl Fn(usize) -> bool) -> (Vec<f32>, Vec<u32>) {
    let mut far = vec![f32::INFINITY; grid.owner.len()];
    let mut back = vec![u32::MAX; grid.owner.len()];
    let mut heap = BinaryHeap::new();
    for &k in from {
        far[k] = 0.0;
        heap.push(Step(0.0, k));
    }
    let (w, h) = (grid.w as i64, grid.h as i64);
    while let Some(Step(d, k)) = heap.pop() {
        if d > far[k] {
            continue;
        }
        let (x, y) = ((k % grid.w) as i64, (k / grid.w) as i64);
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= w || ny >= h {
                continue;
            }
            let n = (ny * w + nx) as usize;
            if !open(n) {
                continue;
            }
            // not across a corner: both cells beside the diagonal are floor
            if dx != 0 && dy != 0 && (!open((y * w + nx) as usize) || !open((ny * w + x) as usize)) {
                continue;
            }
            let step = d + grid.cell * if dx != 0 && dy != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
            if step < far[n] {
                far[n] = step;
                back[n] = k as u32;
                heap.push(Step(step, n));
            }
        }
    }
    (far, back)
}

/// How deep each cell of a room lies in it: how far it is from the nearest cell that is not the
/// room, over the room's own part of the grid.
struct Depth {
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    far: Vec<f32>,
}

impl Depth {
    fn of(grid: &Grid, label: u32, min: Vec2, max: Vec2) -> Depth {
        let lo = ((min - grid.origin) / grid.cell).floor() - Vec2::splat(2.0);
        let hi = ((max - grid.origin) / grid.cell).ceil() + Vec2::splat(2.0);
        let (x0, y0) = (lo.x.max(0.0) as usize, lo.y.max(0.0) as usize);
        let (x1, y1) = ((hi.x.max(0.0) as usize).min(grid.w), (hi.y.max(0.0) as usize).min(grid.h));
        let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        let mut far: Vec<f32> = (0..w * h).map(|k| if grid.owner[(y0 + k / w) * grid.w + x0 + k % w] == label { f32::INFINITY } else { 0.0 }).collect();
        let (one, across) = (grid.cell, grid.cell * std::f32::consts::SQRT_2);
        // twice over the cells, there and back: each takes the least of its neighbours plus a step
        let mut pass = |order: &mut dyn Iterator<Item = usize>, back: bool| {
            for k in order {
                let (x, y) = ((k % w) as i64, (k / w) as i64);
                let s = if back { 1 } else { -1 };
                for (dx, dy, step) in [(s, 0, one), (0, s, one), (s, s, across), (-s, s, across)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                        let n = far[ny as usize * w + nx as usize] + step;
                        if n < far[k] {
                            far[k] = n;
                        }
                    }
                }
            }
        };
        pass(&mut (0..w * h), false);
        pass(&mut (0..w * h).rev(), true);
        for d in &mut far {
            if !d.is_finite() {
                *d = 0.0;
            }
        }
        Depth { x0, y0, w, h, far }
    }

    fn at(&self, grid: &Grid, cell: usize) -> f32 {
        let (x, y) = (cell % grid.w, cell / grid.w);
        if x < self.x0 || y < self.y0 || x >= self.x0 + self.w || y >= self.y0 + self.h { 0.0 } else { self.far[(y - self.y0) * self.w + x - self.x0] }
    }
}

/// The storeys of the rooms: the heights of their floors, those within `apart` taken together.
fn levels(rooms: &[Room], apart: f32) -> Vec<f32> {
    let mut floors: Vec<f32> = rooms.iter().map(|r| r.min.z).collect();
    floors.sort_by(f32::total_cmp);
    let mut out: Vec<f32> = Vec::new();
    for z in floors {
        if out.last().is_none_or(|last| z - last > apart) {
            out.push(z);
        }
    }
    out
}

/// Walks the storeys of `rooms` with their `doors` and `stairs`, on a grid of `cell` metres (made
/// coarser for a storey too large for it).
pub fn walk(rooms: &[Room], doors: &[Way], stairs: &[Way], cell: f32) -> Walked {
    const MOST: f32 = 3.0e6;
    let mut out = Walked {
        escape: vec![None; rooms.len()],
        widest: vec![0.0; rooms.len()],
        area: vec![0.0; rooms.len()],
        doors: vec![0; rooms.len()],
        at_door: vec![Vec::new(); doors.len()],
        compartments: Vec::new(),
    };
    for level in levels(rooms, 1.0) {
        let here: Vec<usize> = (0..rooms.len()).filter(|&k| (rooms[k].min.z - level).abs() <= 1.0 && rooms[k].min.z >= level - 1e-3).collect();
        let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        for &k in &here {
            lo = lo.min(rooms[k].min.truncate());
            hi = hi.max(rooms[k].max.truncate());
        }
        let size = hi - lo + Vec2::splat(4.0);
        let cell = cell.max((size.x * size.y / MOST).sqrt());
        let mut grid = Grid { origin: lo - Vec2::splat(2.0), cell, w: (size.x / cell).ceil() as usize + 1, h: (size.y / cell).ceil() as usize + 1, owner: Vec::new() };
        grid.owner = vec![NONE; grid.w * grid.h];
        for &k in &here {
            for t in &rooms[k].shape {
                let (min, max) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
                let cells: Vec<usize> = grid.cells(min, max).collect();
                for c in cells {
                    if grid.owner[c] == NONE && in_triangle(grid.centre(c), t) {
                        grid.owner[c] = k as u32 + 1;
                    }
                }
            }
            out.area[k] = grid.owner.iter().filter(|&&o| o == k as u32 + 1).count() as f32 * cell * cell;
        }
        // the largest circle in each room: how far its cells are from what is not the room
        let depths: std::collections::HashMap<usize, Depth> = here.iter().map(|&k| (k, Depth::of(&grid, k as u32 + 1, rooms[k].min.truncate(), rooms[k].max.truncate()))).collect();
        for &k in &here {
            // a cell's centre lies half a cell from the edge of the cell beside the room
            out.widest[k] = (2.0 * depths[&k].far.iter().copied().fold(0.0f32, f32::max) - cell).max(0.0);
        }
        // the doors of this storey: their doorway is floor, and they join the rooms beside them
        let mut exits: Vec<usize> = Vec::new();
        let mut joins: Vec<(usize, usize, bool)> = Vec::new();
        for (d, door) in doors.iter().enumerate() {
            if door.min.z < level - 0.6 || door.min.z > level + 1.0 {
                continue;
            }
            let reach = Vec2::splat(0.45);
            let (min, max) = (door.min.truncate() - reach, door.max.truncate() + reach);
            let mut beside: Vec<usize> = grid.cells(min, max).map(|c| grid.owner[c]).filter(|&o| o != NONE && o != DOOR).map(|o| o as usize - 1).collect();
            beside.sort_unstable();
            beside.dedup();
            if beside.is_empty() {
                continue;
            }
            let doorway: Vec<usize> = grid.cells(min, max).filter(|&c| grid.owner[c] == NONE).collect();
            for &c in &doorway {
                grid.owner[c] = DOOR;
            }
            for &r in &beside {
                out.doors[r] += 1;
            }
            match beside[..] {
                // a room on one side only: the door leads outside
                [_] => exits.extend(grid.cells(min, max).filter(|&c| grid.walkable(c))),
                [a, b, ..] => joins.push((a, b, door.fire)),
                [] => {}
            }
            // the room there is at each side of the door
            for &r in beside.iter().take(2) {
                let Some(depth) = depths.get(&r) else { continue };
                let near = Vec2::splat(1.5);
                let widest = grid.cells(door.min.truncate() - near, door.max.truncate() + near).filter(|&c| grid.owner[c] == r as u32 + 1).map(|c| depth.at(&grid, c)).fold(0.0f32, f32::max);
                out.at_door[d].push((r, (2.0 * widest - cell).max(0.0)));
            }
        }
        for stair in stairs {
            if stair.max.z < level - 0.3 || stair.min.z > level + 0.3 {
                continue;
            }
            let reach = Vec2::splat(0.3);
            exits.extend(grid.cells(stair.min.truncate() - reach, stair.max.truncate() + reach).filter(|&c| grid.walkable(c)));
        }
        // how far every cell of the floor is from a way out
        let (far, back) = spread(&grid, &exits, |c| grid.walkable(c));
        for &k in &here {
            let label = k as u32 + 1;
            let worst = (0..grid.owner.len()).filter(|&c| grid.owner[c] == label && far[c].is_finite()).max_by(|&a, &b| far[a].total_cmp(&far[b]));
            let Some(worst) = worst else { continue };
            // the walk back to the way out, then pulled straight where the floor lets it
            let mut cells = vec![worst];
            while let Some(&last) = cells.last() {
                if back[last] == u32::MAX || cells.len() > grid.owner.len() {
                    break;
                }
                cells.push(back[last] as usize);
            }
            let points: Vec<Vec2> = cells.iter().map(|&c| grid.centre(c)).collect();
            let mut pulled = vec![points[0]];
            let mut at = 0;
            while at + 1 < points.len() {
                let mut next = at + 1;
                for far in (at + 2..points.len()).rev() {
                    if grid.clear(points[at], points[far]) {
                        next = far;
                        break;
                    }
                }
                pulled.push(points[next]);
                at = next;
            }
            let length: f32 = pulled.windows(2).map(|p| p[0].distance(p[1])).sum();
            out.escape[k] = Some((length, pulled.into_iter().map(|p| p.extend(level)).collect()));
        }
        // the rooms that hang together through doors that do not hold back fire
        let mut group: Vec<usize> = (0..rooms.len()).collect();
        fn top(group: &mut [usize], k: usize) -> usize {
            let mut at = k;
            while group[at] != at {
                group[at] = group[group[at]];
                at = group[at];
            }
            at
        }
        for &(a, b, fire) in &joins {
            if !fire {
                let (a, b) = (top(&mut group, a), top(&mut group, b));
                group[a] = b;
            }
        }
        let mut together: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
        for &k in &here {
            together.entry(top(&mut group, k)).or_default().push(k);
        }
        out.compartments.extend(together.into_values().map(|rooms| {
            let area = rooms.iter().map(|&r| out.area[r]).sum();
            (rooms, area)
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(x0: f32, y0: f32, x1: f32, y1: f32) -> Room {
        Room::of_box(Vec3::new(x0, y0, 0.0), Vec3::new(x1, y1, 2.7))
    }

    fn door(x0: f32, y0: f32, x1: f32, y1: f32, fire: bool) -> Way {
        Way { min: Vec3::new(x0, y0, 0.0), max: Vec3::new(x1, y1, 2.1), fire }
    }

    #[test]
    fn the_walk_goes_round_the_corner_of_a_room() {
        // an L: 10 by 2 along x, then 2 by 10 up from its far end; the way out at the top
        let mut l = room(0.0, 0.0, 10.0, 2.0);
        l.shape.extend(room(8.0, 2.0, 10.0, 10.0).shape);
        l.max = Vec3::new(10.0, 10.0, 2.7);
        let out = door(8.5, 10.05, 9.5, 10.15, false);
        let walked = walk(&[l], &[out], &[], 0.2);
        let (far, path) = walked.escape[0].clone().unwrap();
        // straight through the wall it would be about 13 m; round the corner it is about 17
        assert!((16.0..19.0).contains(&far), "{far}");
        assert!(path.len() >= 3, "{path:?}");
        // the widest circle is as wide as the legs of the L
        assert!((walked.widest[0] - 2.0).abs() < 0.45, "{}", walked.widest[0]);
        assert!((walked.area[0] - 36.0).abs() < 2.0, "{}", walked.area[0]);
    }

    #[test]
    fn rooms_join_through_doors_and_a_fire_door_parts_them() {
        // three rooms in a row; the door to outside is in the last
        let rooms = [room(0.0, 0.0, 5.0, 4.0), room(5.2, 0.0, 10.0, 4.0), room(10.2, 0.0, 15.0, 4.0)];
        let doors = [door(4.95, 1.5, 5.25, 2.5, false), door(9.95, 1.5, 10.25, 2.5, true), door(14.95, 1.5, 15.25, 2.5, false)];
        let walked = walk(&rooms, &doors, &[], 0.2);
        let far: Vec<f32> = walked.escape.iter().map(|e| e.as_ref().unwrap().0).collect();
        assert!(far[0] > far[1] && far[1] > far[2], "{far:?}");
        assert!((14.0..17.5).contains(&far[0]), "{far:?}");
        assert_eq!(walked.doors, [1, 2, 2]);
        // the fire door parts the first two from the third
        let mut groups: Vec<Vec<usize>> = walked.compartments.iter().map(|c| c.0.clone()).collect();
        groups.sort();
        assert_eq!(groups, [vec![0, 1], vec![2]]);
        // at each side of the first door a circle of nearly the room's width fits
        assert!(walked.at_door[0].iter().all(|&(_, wide)| wide > 3.0), "{:?}", walked.at_door[0]);
        // a room without a door reaches no way out
        let alone = walk(&[room(0.0, 0.0, 3.0, 3.0)], &[], &[], 0.2);
        assert!(alone.escape[0].is_none());
    }

    #[test]
    fn a_stair_is_a_way_out_of_its_storey_only() {
        let up = Room::of_box(Vec3::new(0.0, 0.0, 3.0), Vec3::new(6.0, 4.0, 5.7));
        let stair = Way { min: Vec3::new(5.0, 0.0, 0.0), max: Vec3::new(6.5, 2.0, 3.0), fire: false };
        let walked = walk(&[room(20.0, 0.0, 24.0, 4.0), up], &[], &[stair], 0.2);
        assert!(walked.escape[0].is_none());
        assert!(walked.escape[1].as_ref().is_some_and(|(far, _)| *far < 8.0));
    }
}
