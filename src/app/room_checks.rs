//! The checks of rooms that need their shape, on the meshes of the open models: a room is high
//! enough, and from every corner of it a way out of its storey (a door to outside or a stair) is
//! near enough, walking through the doors from room to room.
//!
//! A room is its box. A door leads between the rooms found half a metre before and behind it;
//! with no room on one side it leads outside. A stair is a way out of its storey. The walk goes in straight lines from a corner of a room to a
//! door of it and from door to door through the rooms between, so it is never longer than the
//! true walk around furniture and walls: a room it flags is too far for certain, a room it
//! passes is to be checked on the drawing.

use super::collab::{field, Checked};
use super::App;
use crate::collab::rules::RoomRules;
use crate::model::Model;
use bim_validator_core::{Finding, RuleSource, Severity, SpecificationResult, ValidationStatus as S};
use glam::Vec3;
use std::collections::BinaryHeap;
use std::sync::Arc;

/// A room, door or stair as the checks see it: its element and its box (metres).
#[derive(Debug, Clone, Copy)]
pub struct Boxed {
    pub id: u64,
    pub min: Vec3,
    pub max: Vec3,
}

impl Boxed {
    fn touches(&self, other: &Boxed, reach: f32) -> bool {
        (self.min - Vec3::splat(reach)).cmple(other.max).all() && (self.max + Vec3::splat(reach)).cmpge(other.min).all()
    }

    fn middle(&self) -> Vec3 {
        (self.min + self.max) / 2.0
    }
}

/// How far each room is from a way out at its worst corner, walking through doors; None for a
/// room no way out is reached from.
pub fn escape_distances(rooms: &[Boxed], doors: &[Boxed], stairs: &[Boxed]) -> Vec<Option<f32>> {
    escape_routes(rooms, doors, stairs).into_iter().map(|r| r.map(|(far, _)| far)).collect()
}

/// As [`escape_distances`], with the walk itself: from the worst corner of the room to its door
/// and on from door to door to the way out.
pub fn escape_routes(rooms: &[Boxed], doors: &[Boxed], stairs: &[Boxed]) -> Vec<Option<(f32, Vec<Vec3>)>> {
    const REACH: f32 = 0.3;
    // the ways in and out: the doors, then the stairs
    let ways: Vec<Boxed> = doors.iter().chain(stairs).copied().collect();
    let inside = |p: Vec3, r: &Boxed| p.cmpge(r.min - Vec3::splat(0.05)).all() && p.cmple(r.max + Vec3::splat(0.05)).all();
    // a door leads from the room at a point before it to the room at a point behind it, half a
    // metre out across its thin side and a metre above its sill; with no room at one of them it
    // leads outside (a room's box is larger than the room, so it is looked at from both sides)
    let sides: Vec<Option<[Vec3; 2]>> = ways
        .iter()
        .enumerate()
        .map(|(k, w)| {
            (k < doors.len()).then(|| {
                let size = w.max - w.min;
                let across = if size.x < size.y { Vec3::X * (size.x / 2.0 + 0.5) } else { Vec3::Y * (size.y / 2.0 + 0.5) };
                let at = Vec3::new(w.middle().x, w.middle().y, w.min.z + 1.0);
                [at - across, at + across]
            })
        })
        .collect();
    let touching: Vec<Vec<usize>> = ways
        .iter()
        .zip(&sides)
        .map(|(w, side)| (0..rooms.len()).filter(|&r| match side { Some(points) => points.iter().any(|&p| inside(p, &rooms[r])), None => w.touches(&rooms[r], REACH) }).collect())
        .collect();
    let outside = |k: usize| match &sides[k] {
        Some(points) => points.iter().any(|&p| !rooms.iter().any(|r| inside(p, r))),
        None => true,
    };
    let of_room: Vec<Vec<usize>> = (0..rooms.len()).map(|r| (0..ways.len()).filter(|&k| touching[k].contains(&r)).collect()).collect();
    // from the ways out back through the rooms: the shortest walk from each door to outside
    let mut far = vec![f32::INFINITY; ways.len()];
    // the door a walk goes on to from each door, towards the way out
    let mut on: Vec<Option<usize>> = vec![None; ways.len()];
    let mut queue = BinaryHeap::new();
    for k in (0..ways.len()).filter(|&k| outside(k)) {
        far[k] = 0.0;
        queue.push((std::cmp::Reverse(0u32), k));
    }
    while let Some((std::cmp::Reverse(d), k)) = queue.pop() {
        let d = d as f32 / 1000.0;
        if d > far[k] + 1e-3 {
            continue;
        }
        for &r in &touching[k] {
            for &j in &of_room[r] {
                let step = d + ways[k].middle().distance(ways[j].middle());
                if step + 1e-3 < far[j] {
                    far[j] = step;
                    on[j] = Some(k);
                    queue.push((std::cmp::Reverse((step * 1000.0) as u32), j));
                }
            }
        }
    }
    rooms
        .iter()
        .enumerate()
        .map(|(r, room)| {
            let floor = room.min.z;
            let corners = [Vec3::new(room.min.x, room.min.y, floor), Vec3::new(room.max.x, room.min.y, floor), Vec3::new(room.min.x, room.max.y, floor), Vec3::new(room.max.x, room.max.y, floor)];
            // each corner by its nearest way; the corner that has furthest to go
            let best = |c: &Vec3| of_room[r].iter().map(|&k| (c.distance(ways[k].middle()) + far[k], k)).min_by(|x, y| x.0.total_cmp(&y.0));
            let (corner, (worst, first)) = corners.iter().filter_map(|c| Some((*c, best(c)?))).max_by(|x, y| x.1 .0.total_cmp(&y.1 .0))?;
            if !worst.is_finite() {
                return None;
            }
            let mut walk = vec![corner];
            let mut at = Some(first);
            while let Some(k) = at.filter(|_| walk.len() <= ways.len() + 1) {
                let m = ways[k].middle();
                walk.push(Vec3::new(m.x, m.y, ways[k].min.z));
                at = on[k];
            }
            Some((worst, walk))
        })
        .collect()
}

fn result(code: &str, name: &str) -> SpecificationResult {
    SpecificationResult {
        name: name.into(),
        identifier: Some(code.into()),
        status: S::Passed,
        applicable_count: 0,
        passed_count: 0,
        failed_count: 0,
        findings: Vec::new(),
        reasons: Vec::new(),
        source: RuleSource::Rooms,
        severity: Severity::Warning,
    }
}

impl App {
    /// The rooms, doors and stairs of open model `which`, as boxes.
    fn room_boxes(model: &Model, which: u32) -> (Vec<Boxed>, Vec<Boxed>, Vec<Boxed>) {
        let (mut rooms, mut doors, mut stairs) = (Vec::new(), Vec::new(), Vec::new());
        let mut seen = std::collections::HashSet::new();
        for layer in &model.layers {
            for &(id, ..) in &layer.elements {
                if model.element_key(id).model != which || !seen.insert(id) {
                    continue;
                }
                let class = model.props.get(&id).and_then(|p| field(p, "IfcType")).unwrap_or("").to_ascii_lowercase();
                let list = match class.as_str() {
                    "ifcspace" => &mut rooms,
                    "ifcdoor" | "ifcdoorstandardcase" => &mut doors,
                    c if c.starts_with("ifcstair") => &mut stairs,
                    _ => continue,
                };
                if let Some(b) = model.element_bounds(id) {
                    list.push(Boxed { id, min: Vec3::from_array(b.min.map(|v| v as f32)), max: Vec3::from_array(b.max.map(|v| v as f32)) });
                }
            }
        }
        (rooms, doors, stairs)
    }

    /// Adds the checks of the rooms' shapes to the reports of the checked models.
    pub(super) fn room_shape_checks(&self, checked: &mut [Checked], rules: &RoomRules) -> std::collections::HashMap<u64, Vec<[f32; 3]>> {
        let mut routes = std::collections::HashMap::new();
        let Some(model) = self.model.clone() else { return routes };
        for c in checked.iter_mut() {
            let Ok(report) = &mut c.report else { continue };
            let (rooms, doors, stairs) = Self::room_boxes(&model, c.model as u32);
            let finding = |room: &Boxed, facet: &str, reason: String| Finding {
                express_id: model.element_key(room.id).source as u32,
                global_id: model.props.get(&room.id).and_then(|p| field(p, "GlobalId")).map(str::to_string),
                entity_type: "IFCSPACE".into(),
                facet: facet.into(),
                status: S::Failed,
                reason,
                name: model.props.get(&room.id).and_then(|p| field(p, "Name")).map(str::to_string),
                storey: None,
            };
            let mut height = result("RMT-HEIGHT", "Room height");
            let mut escape = result("RMT-ESCAPE", "Walk to a way out");
            if rooms.is_empty() {
                for r in [&mut height, &mut escape] {
                    r.status = S::NotCheckable;
                    r.reasons.push("The model has no rooms with a shape".into());
                }
            }
            for room in &rooms {
                let h = room.max.z - room.min.z;
                height.applicable_count += 1;
                if h + 1e-3 < rules.room_height {
                    height.failed_count += 1;
                    height.findings.push(finding(room, "height", format!("The room is {:.2} m high, less than {:.2} m", h, rules.room_height)));
                } else {
                    height.passed_count += 1;
                }
            }
            // a wheelchair turns in each room: its narrower side holds the circle
            let mut turning = result("RMT-TURN", "Turning circle in rooms");
            for room in &rooms {
                let narrow = (room.max.x - room.min.x).min(room.max.y - room.min.y);
                turning.applicable_count += 1;
                if narrow + 1e-3 < rules.turning_circle {
                    turning.failed_count += 1;
                    turning.findings.push(finding(room, "turning circle", format!("The room is {:.2} m wide, less than the turning circle of {:.2} m", narrow, rules.turning_circle)));
                } else {
                    turning.passed_count += 1;
                }
            }
            if rooms.is_empty() {
                turning.status = S::NotCheckable;
                turning.reasons.push("The model has no rooms with a shape".into());
            } else if turning.failed_count > 0 {
                turning.status = S::Failed;
            }
            for (room, walk) in rooms.iter().zip(escape_routes(&rooms, &doors, &stairs)) {
                escape.applicable_count += 1;
                // the walk of each room, to draw when its finding is picked
                if let Some((_, path)) = &walk {
                    routes.insert(room.id, path.iter().map(|p| p.to_array()).collect());
                }
                match walk.map(|w| w.0) {
                    Some(d) if d <= rules.escape_distance => escape.passed_count += 1,
                    Some(d) => {
                        escape.failed_count += 1;
                        escape.findings.push(finding(room, "escape", format!("From its farthest corner the walk to a way out is at least {d:.1} m, more than {:.0} m", rules.escape_distance)));
                    }
                    None => {
                        escape.failed_count += 1;
                        // a room without a door of its own, or whose doors lead only to rooms with no way on
                        let doors_of = doors.iter().filter(|d| d.touches(room, 0.3)).count();
                        let why = if doors_of == 0 {
                            "The room has no door".to_string()
                        } else {
                            format!("Its {doors_of} doors lead only to rooms with no way out: is a corridor or hall missing as a room?")
                        };
                        escape.findings.push(finding(room, "escape", why));
                    }
                }
            }
            for r in [&mut height, &mut escape] {
                if r.failed_count > 0 {
                    r.status = S::Failed;
                }
            }
            let report = Arc::make_mut(report);
            report.specifications.push(height);
            report.specifications.push(escape);
            report.specifications.push(turning);
        }
        routes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(id: u64, lo: [f32; 3], hi: [f32; 3]) -> Boxed {
        Boxed { id, min: Vec3::from_array(lo), max: Vec3::from_array(hi) }
    }

    #[test]
    fn the_walk_goes_from_room_to_room_through_the_doors() {
        // three rooms in a row, 10 m each; a door between each two and one to outside at the end
        let rooms = [b(1, [0.0, 0.0, 0.0], [10.0, 5.0, 2.6]), b(2, [10.0, 0.0, 0.0], [20.0, 5.0, 2.6]), b(3, [20.0, 0.0, 0.0], [30.0, 5.0, 2.6])];
        let door = |id, x: f32| b(id, [x - 0.05, 2.0, 0.0], [x + 0.05, 2.9, 2.3]);
        let doors = [door(10, 10.0), door(11, 20.0), door(12, 30.0)];
        let walks = escape_distances(&rooms, &doors, &[]);
        let far = walks.iter().map(|w| w.unwrap()).collect::<Vec<_>>();
        // the first room: from its far corner to the first door, then 20 m on
        assert!(far[0] > 30.0 && far[0] < 32.0, "{far:?}");
        assert!(far[2] < far[1] && far[1] < far[0], "{far:?}");
        // the walk of the first room: from a far corner through the three doors
        let routes = escape_routes(&rooms, &doors, &[]);
        let walk = &routes[0].as_ref().unwrap().1;
        assert_eq!(walk.len(), 4, "{walk:?}");
        assert!(walk[0].x.abs() < 1e-6 && (walk[1].x - 10.0).abs() < 0.1 && (walk[3].x - 30.0).abs() < 0.1, "{walk:?}");
        // without the door to outside no way out is found
        assert!(escape_distances(&rooms, &doors[..2], &[]).iter().all(|w| w.is_none()));
        // a stair in the middle room is a way out too
        let stair = b(20, [14.0, 1.0, 0.0], [16.0, 4.0, 3.0]);
        let with_stair = escape_distances(&rooms, &doors[..2], &[stair]);
        assert!(with_stair.iter().all(|w| w.is_some_and(|d| d < 22.0)), "{with_stair:?}");
        // two L-shaped rooms whose boxes overlap: the door in the front wall where both boxes
        // reach still leads outside, for the room behind it has nothing in front
        let overlapping = [b(1, [0.0, 0.0, 0.0], [10.0, 10.0, 2.6]), b(2, [5.0, 0.0, 0.0], [15.0, 10.0, 2.6])];
        let front = b(13, [6.0, -0.05, 0.0], [7.0, 0.05, 2.3]);
        assert!(escape_distances(&overlapping, &[front], &[]).iter().all(|w| w.is_some()));
    }
}
