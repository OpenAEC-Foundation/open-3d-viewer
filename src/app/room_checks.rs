//! The checks of rooms that need their shape, on the meshes of the open models: a room is high
//! enough, a wheelchair turns in it and at its doors, from every point of it a way out of its
//! storey (a door to outside or a stair) is near enough, and the rooms that hang together
//! through doors that do not hold back fire are not too large together.
//!
//! The rooms are laid on a grid by their own shape and walked over (see
//! [`crate::collab::floor`]): the walk goes through the doors and round the corners, as far as
//! the rooms are modelled. Furniture is not in the way, so the true walk can be longer.

use super::collab::{field, Checked};
use super::App;
use crate::collab::floor::{self, Room, Way};
use crate::collab::rules::RoomRules;
use crate::model::Model;
use bim_validator_core::{Finding, RuleSource, Severity, SpecificationResult, ValidationStatus as S};
use glam::Vec3;
use std::sync::Arc;

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

/// Whether an element's properties name a fire rating.
pub(super) fn fire_rated(props: &str) -> bool {
    const KEY: &str = "FireRating\":\"";
    props.match_indices(KEY).any(|(at, _)| {
        let value = &props[at + KEY.len()..];
        let value = value[..value.find('"').unwrap_or(0)].trim().to_ascii_lowercase();
        !value.is_empty() && !["0", "-", "n/a", "none", "geen"].contains(&value.as_str())
    })
}

/// The rooms, doors and stairs of a model as the walk takes them, each with its element.
pub(super) struct Floors {
    pub rooms: Vec<(u64, Room)>,
    pub doors: Vec<(u64, Way)>,
    pub stairs: Vec<Way>,
}

impl App {
    /// The rooms, doors and stairs of open model `which`: the rooms with their shape seen from
    /// above.
    pub(super) fn floors(model: &Model, which: u32) -> Floors {
        let mut out = Floors { rooms: Vec::new(), doors: Vec::new(), stairs: Vec::new() };
        let mut seen = std::collections::HashSet::new();
        for layer in &model.layers {
            for &(id, ..) in &layer.elements {
                if model.element_key(id).model != which || !seen.insert(id) {
                    continue;
                }
                let props = model.props.get(&id).map(String::as_str).unwrap_or("");
                let class = field(props, "IfcType").unwrap_or("").to_ascii_lowercase();
                if class != "ifcspace" && !class.starts_with("ifcdoor") && !class.starts_with("ifcstair") {
                    continue;
                }
                let Some(b) = model.element_bounds(id) else { continue };
                let (min, max) = (Vec3::from_array(b.min.map(|v| v as f32)), Vec3::from_array(b.max.map(|v| v as f32)));
                if class == "ifcspace" {
                    // its triangles that lie flat: its floor and its ceiling
                    let mut shape = Vec::new();
                    for (l, first, count) in model.element_ranges(id) {
                        let layer = &model.layers[l];
                        let point = |i: u32| layer.positions.get(3 * i as usize..3 * i as usize + 3).map(|p| Vec3::new(p[0], p[1], p[2]));
                        for tri in layer.indices.get(first as usize..(first + count) as usize).unwrap_or(&[]).chunks_exact(3) {
                            let (Some(a), Some(b), Some(c)) = (point(tri[0]), point(tri[1]), point(tri[2])) else { continue };
                            let n = (b - a).cross(c - a);
                            if n.length() > 1e-9 && n.z.abs() > 0.7 * n.length() {
                                shape.push([a.truncate(), b.truncate(), c.truncate()]);
                            }
                        }
                    }
                    out.rooms.push((id, if shape.is_empty() { Room::of_box(min, max) } else { Room { min, max, shape } }));
                } else if class.starts_with("ifcdoor") {
                    out.doors.push((id, Way { min, max, fire: fire_rated(props) }));
                } else {
                    out.stairs.push(Way { min, max, fire: false });
                }
            }
        }
        out
    }

    /// Adds the checks of the rooms' shapes to the reports of the checked models. Returns the
    /// walk of each room to its way out, by the room's element.
    pub(super) fn room_shape_checks(&self, checked: &mut [Checked], rules: &RoomRules) -> std::collections::HashMap<u64, Vec<[f32; 3]>> {
        let mut routes = std::collections::HashMap::new();
        let Some(model) = self.model.clone() else { return routes };
        for c in checked.iter_mut() {
            let Ok(report) = &mut c.report else { continue };
            let floors = Self::floors(&model, c.model as u32);
            let rooms: Vec<Room> = floors.rooms.iter().map(|r| r.1.clone()).collect();
            let doors: Vec<Way> = floors.doors.iter().map(|d| d.1).collect();
            let walked = floor::walk(&rooms, &doors, &floors.stairs, 0.2);
            let name = |id: u64| model.props.get(&id).and_then(|p| field(p, "Name")).map(str::to_string);
            let finding = |id: u64, class: &str, facet: &str, reason: String| Finding {
                express_id: model.element_key(id).source as u32,
                global_id: model.props.get(&id).and_then(|p| field(p, "GlobalId")).map(str::to_string),
                entity_type: class.into(),
                facet: facet.into(),
                status: S::Failed,
                reason,
                name: name(id),
                storey: None,
            };
            let mut height = result("RMT-HEIGHT", "Room height");
            let mut escape = result("RMT-ESCAPE", "Walk to a way out");
            let mut turning = result("RMT-TURN", "Turning circle in rooms");
            let mut at_door = result("RMT-DOORSPACE", "Free space at doors");
            let mut compartment = result("RMT-COMPARTMENT", "Floor of a fire compartment");
            for (k, (id, room)) in floors.rooms.iter().enumerate() {
                let h = room.max.z - room.min.z;
                height.applicable_count += 1;
                if h + 1e-3 < rules.room_height {
                    height.failed_count += 1;
                    height.findings.push(finding(*id, "IFCSPACE", "height", format!("The room is {:.2} m high, less than {:.2} m", h, rules.room_height)));
                } else {
                    height.passed_count += 1;
                }
                // a wheelchair turns in the room: the largest circle that fits in it (the grid
                // sees it a cell short at most)
                turning.applicable_count += 1;
                if walked.widest[k] + 0.2 < rules.turning_circle {
                    turning.failed_count += 1;
                    turning.findings.push(finding(*id, "IFCSPACE", "turning circle", format!("The largest circle that fits in the room is {:.2} m, less than the turning circle of {:.2} m", walked.widest[k], rules.turning_circle)));
                } else {
                    turning.passed_count += 1;
                }
                if rules.escape_distance <= 0.0 {
                    continue;
                }
                escape.applicable_count += 1;
                match &walked.escape[k] {
                    Some((far, path)) => {
                        routes.insert(*id, path.iter().map(|p| p.to_array()).collect());
                        if *far <= rules.escape_distance {
                            escape.passed_count += 1;
                        } else {
                            escape.failed_count += 1;
                            escape.findings.push(finding(*id, "IFCSPACE", "escape", format!("From its farthest point the walk to a way out is {far:.1} m, more than {:.0} m", rules.escape_distance)));
                        }
                    }
                    None => {
                        escape.failed_count += 1;
                        let why = match walked.doors[k] {
                            0 => "The room has no door".to_string(),
                            n => format!("Its {n} doors lead only to rooms with no way out: is a corridor or hall missing as a room?"),
                        };
                        escape.findings.push(finding(*id, "IFCSPACE", "escape", why));
                    }
                }
            }
            if rules.door_space > 0.0 {
                for (d, (id, _)) in floors.doors.iter().enumerate() {
                    for &(room, wide) in &walked.at_door[d] {
                        at_door.applicable_count += 1;
                        if wide + 0.2 < rules.door_space {
                            at_door.failed_count += 1;
                            let of = name(floors.rooms[room].0).unwrap_or_default();
                            at_door.findings.push(finding(*id, "IFCDOOR", "free space", format!("At the door in room {of} a circle of {wide:.2} m fits, less than {:.2} m", rules.door_space)));
                        } else {
                            at_door.passed_count += 1;
                        }
                    }
                }
            }
            if rules.compartment_area > 0.0 {
                for (together, area) in &walked.compartments {
                    compartment.applicable_count += 1;
                    if *area > rules.compartment_area {
                        compartment.failed_count += 1;
                        // on its largest room
                        let largest = together.iter().copied().max_by(|&a, &b| walked.area[a].total_cmp(&walked.area[b])).unwrap_or(together[0]);
                        compartment.findings.push(finding(
                            floors.rooms[largest].0,
                            "IFCSPACE",
                            "compartment",
                            format!("{} rooms hang together through doors without a fire rating: {area:.0} m² of floor, more than {:.0} m²", together.len(), rules.compartment_area),
                        ));
                    } else {
                        compartment.passed_count += 1;
                    }
                }
            }
            let report = Arc::make_mut(report);
            for mut r in [height, escape, turning, at_door, compartment] {
                if floors.rooms.is_empty() {
                    r.status = S::NotCheckable;
                    r.reasons.push("The model has no rooms with a shape".into());
                } else if r.failed_count > 0 {
                    r.status = S::Failed;
                }
                report.specifications.push(r);
            }
        }
        routes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fire_rating_is_read_from_an_elements_properties() {
        assert!(fire_rated(r#"{"IfcType":"IfcDoor","FireRating":"EW30"}"#));
        assert!(fire_rated(r#"{"Pset_DoorCommon.FireRating":"30"}"#));
        assert!(!fire_rated(r#"{"FireRating":""}"#) && !fire_rated(r#"{"FireRating":"-"}"#) && !fire_rated(r#"{"Name":"deur"}"#));
    }
}
