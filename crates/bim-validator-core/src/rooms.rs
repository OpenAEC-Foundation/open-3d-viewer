//! Checks of rooms, doors and fire separations on the entity graph, without geometry: every
//! storey with elements has its rooms, a room has a number unique on its storey, a door is wide
//! and high enough, a door in a fire-rated wall is rated too, and an opening through a
//! fire-rated wall or slab is filled or sealed. Codes are `RMT-` plus a topic. The checks that
//! need the shape of a room (its height, the walk to a way out) are the viewer's.
use crate::checks::{tick, Model, Rule};
use crate::graph::{number, text, Node};
use crate::{RoomOptions, RuleSource, Severity, SpecificationResult};
use ifc_lite_core::AttributeValue as A;
use std::collections::BTreeMap;

fn rule<'a>(m: &Model<'a>, code: &str, name: &str) -> Rule<'a> {
    Rule::new(m.g, RuleSource::Rooms, Severity::Warning, code, name)
}

pub fn run(m: &Model, o: &RoomOptions) -> Result<Vec<SpecificationResult>, String> {
    let mut out = vec![rooms_per_storey(m)?];
    out.push(room_numbers(m)?);
    out.push(door_sizes(m, o)?);
    out.push(thresholds(m, o)?);
    out.push(ramps(m, o)?);
    out.push(fire_doors(m)?);
    out.push(fire_openings(m)?);
    Ok(out)
}

/// A fire rating in minutes: a number, or a text that starts with one (`60`, `60 min`,
/// `EI 30`, `WBDBO 60`).
pub fn minutes(a: &A) -> Option<f64> {
    if let Some(v) = number(a) {
        return Some(v);
    }
    let s = text(a)?;
    let digits: String = s.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// The fire rating an element (or its type) gives in its common property set, in minutes.
fn rating(m: &Model, n: &Node) -> Option<f64> {
    m.property(n, |s| s.starts_with("Pset_") && s.ends_with("Common"), "FireRating").and_then(|(_, v)| minutes(v)).filter(|v| *v > 0.)
}

fn rooms<'a>(m: &Model<'a>) -> Vec<&'a Node> {
    m.g.nodes.values().filter(|n| m.is(n, "IFCSPACE")).collect()
}

/// RMT-ROOMS: each storey that holds elements has rooms.
fn rooms_per_storey(m: &Model) -> Result<SpecificationResult, String> {
    let mut r = rule(m, "RMT-ROOMS", "Rooms on every storey");
    let mut with_rooms = BTreeMap::new();
    for space in rooms(m) {
        if let Some(storey) = m.g.storey_of(space.id) {
            *with_rooms.entry(storey).or_insert(0usize) += 1;
        }
    }
    let mut with_elements = BTreeMap::new();
    for n in &m.elements {
        tick(m.cancel)?;
        if let Some(storey) = m.g.storey_of(n.id) {
            *with_elements.entry(storey).or_insert(0usize) += 1;
        }
    }
    for (storey, count) in with_elements {
        let Some(node) = m.g.nodes.get(&storey) else { continue };
        let problems = if with_rooms.contains_key(&storey) { vec![] } else { vec![format!("The storey holds {count} elements and no rooms (IfcSpace)")] };
        r.check(node, "rooms", problems);
    }
    Ok(r.finish())
}

/// RMT-NUMBER: a room has a number (its Name), not shared with another room of its storey.
fn room_numbers(m: &Model) -> Result<SpecificationResult, String> {
    let mut r = rule(m, "RMT-NUMBER", "Room numbers");
    let spaces = rooms(m);
    let mut seen: BTreeMap<(Option<u32>, String), usize> = BTreeMap::new();
    for space in &spaces {
        if let Some(name) = m.g.name(space) {
            *seen.entry((m.g.storey_of(space.id), name.trim().to_string())).or_default() += 1;
        }
    }
    for space in spaces {
        tick(m.cancel)?;
        let problems = match m.g.name(space) {
            None => vec!["The room has no number (Name)".into()],
            Some(name) if seen.get(&(m.g.storey_of(space.id), name.trim().to_string())).is_some_and(|c| *c > 1) => vec![format!("Room number '{}' is used more than once on its storey", name.trim())],
            Some(_) => vec![],
        };
        r.check(space, "room", problems);
    }
    Ok(r.finish())
}

/// RMT-DOOR: a door's overall width and height are at least what the rules ask. The overall
/// size takes the frame in; the free passage is a little less.
fn door_sizes(m: &Model, o: &RoomOptions) -> Result<SpecificationResult, String> {
    let mut r = rule(m, "RMT-DOOR", "Door width and height");
    let scale = m.length_unit().1.unwrap_or(1.0);
    for n in &m.elements {
        tick(m.cancel)?;
        if !m.is(n, "IFCDOOR") {
            continue;
        }
        let size = |name: &str| m.g.attr(n, name).and_then(number).map(|v| v * scale);
        let mut problems = vec![];
        match (size("OverallWidth"), size("OverallHeight")) {
            (None, None) => problems.push("The door gives no OverallWidth and OverallHeight".into()),
            (width, height) => {
                if let Some(w) = width.filter(|w| *w + 1e-6 < o.door_width) {
                    problems.push(format!("Width {:.0} mm is less than {:.0} mm", w * 1000.0, o.door_width * 1000.0));
                }
                if let Some(h) = height.filter(|h| *h + 1e-6 < o.door_height) {
                    problems.push(format!("Height {:.0} mm is less than {:.0} mm", h * 1000.0, o.door_height * 1000.0));
                }
            }
        }
        r.check(n, "door", problems);
    }
    Ok(r.finish())
}

/// RMT-THRESHOLD: a door that gives the height of its threshold (a property named
/// ThresholdHeight, or Drempelhoogte, in any of its property sets) keeps it at or under what the
/// rules ask. Doors that do not give one are not counted.
fn thresholds(m: &Model, o: &RoomOptions) -> Result<SpecificationResult, String> {
    let mut r = rule(m, "RMT-THRESHOLD", "Door thresholds");
    let scale = m.length_unit().1.unwrap_or(1.0);
    for n in &m.elements {
        tick(m.cancel)?;
        if !m.is(n, "IFCDOOR") {
            continue;
        }
        let Some(height) = ["ThresholdHeight", "Drempelhoogte"].iter().find_map(|name| m.property(n, |_| true, name)).and_then(|(_, v)| number(v)) else { continue };
        let height = height * scale;
        let problems = if height > o.threshold + 1e-6 { vec![format!("Threshold {:.0} mm is higher than {:.0} mm", height * 1000.0, o.threshold * 1000.0)] } else { vec![] };
        r.check(n, "threshold", problems);
    }
    Ok(r.finish())
}

/// RMT-RAMP: a ramp flight that gives its slope (Pset_RampFlightCommon.Slope, an angle) is not
/// steeper than the rules ask. A value under 1.6 is read as radians, else as degrees.
fn ramps(m: &Model, o: &RoomOptions) -> Result<SpecificationResult, String> {
    let mut r = rule(m, "RMT-RAMP", "Ramp slope");
    let steepest = (1.0 / o.ramp_run.max(1.0)).atan();
    for n in &m.elements {
        tick(m.cancel)?;
        if !m.is(n, "IFCRAMPFLIGHT") && !m.is(n, "IFCRAMP") {
            continue;
        }
        let Some(slope) = m.property(n, |s| s.starts_with("Pset_Ramp"), "Slope").and_then(|(_, v)| number(v)) else { continue };
        let angle = if slope.abs() < 1.6 { slope.abs() } else { slope.abs().to_radians() };
        let problems = if angle > steepest + 1e-4 { vec![format!("Slope 1 in {:.1} is steeper than 1 in {:.0}", 1.0 / angle.tan(), o.ramp_run)] } else { vec![] };
        r.check(n, "slope", problems);
    }
    Ok(r.finish())
}

/// The wall or slab an element fills an opening of: door → opening → wall.
fn host<'a>(m: &Model<'a>, n: &Node) -> Option<&'a Node> {
    let opening = m.related(n.id, &["IFCRELFILLSELEMENT"]).into_iter().next()?;
    m.related(opening.id, &["IFCRELVOIDSELEMENT"]).into_iter().next()
}

/// RMT-FIREDOOR: a door in a fire-rated wall has a fire rating of its own, not less than the
/// wall's.
fn fire_doors(m: &Model) -> Result<SpecificationResult, String> {
    let mut r = rule(m, "RMT-FIREDOOR", "Doors in fire-rated walls");
    for n in &m.elements {
        tick(m.cancel)?;
        if !m.is(n, "IFCDOOR") {
            continue;
        }
        let Some((wall, needed)) = host(m, n).and_then(|w| Some((w, rating(m, w)?))) else { continue };
        let wall_name = m.g.name(wall).unwrap_or_else(|| format!("#{}", wall.id));
        let problems = match rating(m, n) {
            None => vec![format!("The door has no FireRating; its wall '{wall_name}' is rated {needed:.0} minutes")],
            Some(have) if have < needed => vec![format!("The door is rated {have:.0} minutes, its wall '{wall_name}' {needed:.0} minutes")],
            Some(_) => vec![],
        };
        r.check(n, "fire rating", problems);
    }
    Ok(r.finish())
}

/// RMT-FIREOPENING: an opening through a fire-rated wall or slab is filled (a door, a window)
/// or sealed; one left open breaks the separation.
fn fire_openings(m: &Model) -> Result<SpecificationResult, String> {
    let mut r = rule(m, "RMT-FIREOPENING", "Openings in fire-rated walls and slabs");
    let filled: std::collections::HashSet<u32> = m
        .g
        .nodes
        .values()
        .filter(|n| m.is(n, "IFCELEMENT"))
        .flat_map(|n| m.related(n.id, &["IFCRELFILLSELEMENT"]))
        .map(|o| o.id)
        .collect();
    for opening in m.g.nodes.values().filter(|n| m.is(n, "IFCOPENINGELEMENT")) {
        tick(m.cancel)?;
        let Some(host) = m.related(opening.id, &["IFCRELVOIDSELEMENT"]).into_iter().next() else { continue };
        let Some(needed) = rating(m, host) else { continue };
        let host_name = m.g.name(host).unwrap_or_else(|| format!("#{}", host.id));
        let problems = if filled.contains(&opening.id) { vec![] } else { vec![format!("An opening without a door, window or seal in '{host_name}', rated {needed:.0} minutes: seal it to the same rating")] };
        r.check(opening, "opening", problems);
    }
    Ok(r.finish())
}
