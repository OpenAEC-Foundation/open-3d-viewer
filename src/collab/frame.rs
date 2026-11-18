//! The steel around an opening: whether a window or a door in a wall has a post at each side, a
//! lintel above and (a window) a sill member below. A clash test finds what crosses; this finds
//! what is missing.
//!
//! Openings and steel are taken by their boxes, in plan along the wall: a wall runs along x or
//! along y (the side of its box that is longer), and so does an opening in it.

use glam::Vec3;

/// The sides of an opening, as bits of what is missing.
pub const LEFT: u8 = 1;
pub const RIGHT: u8 = 2;
pub const TOP: u8 = 4;
pub const BOTTOM: u8 = 8;

/// A box: its lowest and highest corner.
pub type Bounds = (Vec3, Vec3);

/// How far (metres) a member may stand back from or reach past the edge of the opening and
/// still count for that side: it lies against the frame, not on it.
const PAST: f32 = 0.05;
/// A door's head lies at least this high (metres) above its foot: a door family often carries a
/// panel or a frame above it up to the ceiling, and a lintel at the head of the leaf counts.
const DOOR_HEAD: f32 = 2.0;

/// The sides of `opening` (a window, or a door: `sill` false) that no box of `steel` frames
/// within `reach` (metres) of its edge, in the plane of `host` (the wall it is in; none: the
/// opening's own plane, `depth` either way).
pub fn missing(opening: Bounds, host: Option<Bounds>, steel: &[Bounds], reach: f32, sill: bool) -> u8 {
    let (lo, hi) = opening;
    // the wall's direction: its longer side in plan; without a wall, the opening's
    let (wlo, whi) = host.unwrap_or(opening);
    let along = if whi.x - wlo.x >= whi.y - wlo.y { 0 } else { 1 };
    let across = 1 - along;
    // steel counts within the wall's thickness and a little more on each side
    let (near, far) = (wlo[across].min(lo[across]) - 0.3, whi[across].max(hi[across]) + 0.3);
    let head = if sill { hi.z } else { hi.z.min(lo.z + DOOR_HEAD) };
    let mut found = 0u8;
    for &(slo, shi) in steel {
        if shi[across] < near || slo[across] > far {
            continue;
        }
        // overlapping the opening's height, or its width, by more than a little
        let beside = slo.z < hi.z - 0.1 && shi.z > lo.z + 0.1;
        let over = slo[along] < hi[along] - PAST && shi[along] > lo[along] + PAST;
        if beside && shi[along] <= lo[along] + PAST && shi[along] >= lo[along] - reach {
            found |= LEFT;
        }
        if beside && slo[along] >= hi[along] - PAST && slo[along] <= hi[along] + reach {
            found |= RIGHT;
        }
        if over && slo.z >= head - PAST && slo.z <= hi.z + reach {
            found |= TOP;
        }
        if over && shi.z <= lo.z + PAST && shi.z >= lo.z - reach {
            found |= BOTTOM;
        }
    }
    let wanted = LEFT | RIGHT | TOP | if sill { BOTTOM } else { 0 };
    wanted & !found
}

/// Whether box `inner` lies within `outer` (a pane in its frame).
pub fn within(inner: Bounds, outer: Bounds) -> bool {
    inner.0.cmpge(outer.0 - Vec3::splat(0.01)).all() && inner.1.cmple(outer.1 + Vec3::splat(0.01)).all() && inner != outer
}

/// What is missing, in words: `post left, lintel`.
pub fn words(missing: u8, dutch: bool) -> String {
    let all = [
        (LEFT, "stijl links", "post left"),
        (RIGHT, "stijl rechts", "post right"),
        (TOP, "latei", "lintel"),
        (BOTTOM, "onderregel", "sill"),
    ];
    all.iter().filter(|(bit, _, _)| missing & bit != 0).map(|(_, nl, en)| if dutch { *nl } else { *en }).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(lo: [f32; 3], hi: [f32; 3]) -> Bounds {
        (Vec3::from_array(lo), Vec3::from_array(hi))
    }

    #[test]
    fn a_window_needs_posts_a_lintel_and_a_sill_and_a_door_no_sill() {
        // a sandwich wall along x, 135 thick; a window 900 wide from 1.0 to 2.8
        let wall = b([0.0, 18.59, 0.0], [12.0, 18.725, 3.0]);
        let window = b([2.61, 18.625, 1.0], [3.51, 18.69, 2.8]);
        let post = |x: f32| b([x - 0.06, 18.47, -0.09], [x, 18.59, 3.01]);
        let posts = [post(2.595), b([3.525, 18.47, -0.09], [3.585, 18.59, 3.01])];
        // posts 75 mm out of the frame, behind the wall: no lintel, no sill
        assert_eq!(missing(window, Some(wall), &posts, 0.25, true), TOP | BOTTOM);
        assert_eq!(words(TOP | BOTTOM, false), "lintel, sill");
        // with both
        let lintel = b([2.535, 18.51, 2.8], [3.585, 18.59, 2.88]);
        let sill = b([2.535, 18.51, 0.92], [3.585, 18.59, 1.0]);
        assert_eq!(missing(window, Some(wall), &[posts[0], posts[1], lintel, sill], 0.25, true), 0);
        // a door asks no sill; a column 90 mm off counts as its post
        let door = b([0.06, 18.625, 0.0], [1.26, 18.69, 2.8]);
        let column = b([-0.19, 18.43, -0.09], [-0.03, 18.59, 3.0]);
        assert_eq!(missing(door, Some(wall), &[column, lintel], 0.25, false), RIGHT | TOP, "the lintel is over the window, not the door");
        // a door with a panel above it to the ceiling: the lintel at the head of the leaf counts
        let tall = b([0.06, 18.625, 0.0], [1.26, 18.69, 4.0]);
        let head = b([0.0, 18.51, 2.38], [1.32, 18.59, 2.46]);
        assert_eq!(missing(tall, Some(wall), &[column, head], 0.25, false), RIGHT);
        assert_eq!(missing(tall, Some(wall), &[column, b([0.0, 18.51, 1.2], [1.32, 18.59, 1.28])], 0.25, false), RIGHT | TOP, "not a member halfway up");
        // steel further than the reach, or in another wall, does not count
        assert_eq!(missing(window, Some(wall), &[post(2.30)], 0.25, true), LEFT | RIGHT | TOP | BOTTOM);
        let other_wall = b([2.535, 25.0, 2.8], [3.585, 25.1, 2.88]);
        assert_eq!(missing(window, Some(wall), &[other_wall], 0.25, true), LEFT | RIGHT | TOP | BOTTOM);
    }

    #[test]
    fn an_opening_in_a_wall_along_y_takes_its_sides_along_y() {
        let wall = b([-3.3, 0.0, 0.0], [-3.1, 20.0, 4.0]);
        let door = b([-3.32, 17.106, 0.0], [-3.2, 18.14, 2.382]);
        let lintel = b([-3.3, 17.106, 2.382], [-3.22, 18.438, 2.462]);
        let post = b([-3.3, 18.15, -0.09], [-3.22, 18.21, 3.0]);
        assert_eq!(missing(door, Some(wall), &[lintel, post], 0.25, false), LEFT);
        // a pane in its frame is not an opening of its own
        assert!(within(b([2.7, 18.63, 1.1], [3.4, 18.66, 2.7]), b([2.61, 18.625, 1.0], [3.51, 18.69, 2.8])));
    }
}
