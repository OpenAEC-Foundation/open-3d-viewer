//! Laying a model on another by the grid lines they share: the architect's model and the
//! structural engineer's of one building often each have their own zero point, but the same grids
//! by name. The move that takes the grids of one onto those of the other takes the whole model.
//!
//! Only a move in plan is found, no turn: when the grids of the two run in other directions the
//! models are left where they are. A grid known by its place alone (both ends in one point, as
//! some readers give them) counts by the direction of its namesake in the other model.

use crate::model::GridLine;
use glam::{DVec2, DVec3};

/// Grids this far (metres) from where the others put them do not count: a namesake that is
/// another line, or a grid moved in one model and not in the other.
const SPREAD: f64 = 0.02;
/// Directions this far apart (radians) are not the same: the models are turned against each other.
const TURNED: f64 = 0.002;

/// How a model was laid on another.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aligned {
    /// The move in plan (metres; z is 0).
    pub by: DVec3,
    /// The grids that agree with the move, and those of the same name that do not.
    pub grids: usize,
    pub left_out: usize,
}

struct Line {
    point: DVec2,
    /// Unit direction; none for a grid given as one point.
    direction: Option<DVec2>,
}

fn line(grid: &GridLine) -> Line {
    let (a, b) = (DVec2::new(grid.a[0] as f64, grid.a[1] as f64), DVec2::new(grid.b[0] as f64, grid.b[1] as f64));
    let d = b - a;
    Line { point: a, direction: (d.length() > 1e-6).then(|| d.normalize()) }
}

/// The move that lays the grids of `added` on those of `reference`, by their names: found when
/// at least two grids in two directions agree within [`SPREAD`], else none.
pub fn on_grids(reference: &[GridLine], added: &[GridLine]) -> Option<Aligned> {
    // per name shared once in each: the normal of the line, and how far the added grid lies
    // from the reference along it (the move must make that zero)
    let mut pairs: Vec<(DVec2, f64, DVec2, DVec2)> = Vec::new();
    for r in reference {
        let (mut same, mut there) = (reference.iter().filter(|g| g.name == r.name), added.iter().filter(|g| g.name == r.name));
        let (Some(_), None, Some(a), None) = (same.next(), same.next(), there.next(), there.next()) else { continue };
        let (lr, la) = (line(r), line(a));
        let direction = match (lr.direction, la.direction) {
            (Some(dr), Some(da)) => {
                if dr.perp_dot(da).abs() > TURNED {
                    return None;
                }
                dr
            }
            (Some(d), None) | (None, Some(d)) => d,
            (None, None) => continue,
        };
        let normal = direction.perp();
        pairs.push((normal, normal.dot(lr.point - la.point), lr.point, la.point));
    }
    let solve = |use_: &dyn Fn(usize) -> bool| -> Option<DVec2> {
        // least squares of normal · t = gap
        let (mut m, mut v) = ([0.0f64; 3], DVec2::ZERO);
        for (k, (n, gap, _, _)) in pairs.iter().enumerate().filter(|(k, _)| use_(*k)) {
            let _ = k;
            m[0] += n.x * n.x;
            m[1] += n.x * n.y;
            m[2] += n.y * n.y;
            v += *n * *gap;
        }
        let det = m[0] * m[2] - m[1] * m[1];
        (det.abs() > 1e-6).then(|| DVec2::new(m[2] * v.x - m[1] * v.y, m[0] * v.y - m[1] * v.x) / det)
    };
    let fits = |t: DVec2, k: usize| (pairs[k].0.dot(t) - pairs[k].1).abs() <= SPREAD;
    // the move most grids agree on: each pair of crossing grids proposes one, the one with the
    // most agreeing grids is refined over those
    let mut best: Option<(usize, DVec2)> = None;
    for i in 0..pairs.len() {
        for j in i + 1..pairs.len() {
            let Some(t) = solve(&|k| k == i || k == j) else { continue };
            let agree = (0..pairs.len()).filter(|&k| fits(t, k)).count();
            if best.is_none_or(|(n, _)| agree > n) {
                best = Some((agree, t));
            }
        }
        if best.is_some_and(|(n, _)| n == pairs.len()) {
            break;
        }
    }
    let (_, rough) = best?;
    let t = solve(&|k| fits(rough, k))?;
    let grids = (0..pairs.len()).filter(|&k| fits(t, k)).count();
    (grids >= 2).then(|| Aligned { by: DVec3::new(t.x, t.y, 0.0), grids, left_out: pairs.len() - grids })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(name: &str, a: [f32; 2], b: [f32; 2]) -> GridLine {
        GridLine { name: name.into(), a: [a[0], a[1], 0.0], b: [b[0], b[1], 0.0] }
    }

    #[test]
    fn a_model_is_laid_on_another_by_the_grids_they_share() {
        // the structural model: grids as lines
        let structure = [grid("S1", [-77.44, -17.0], [-77.44, 24.0]), grid("K1", [-3.21, 21.09], [-3.21, -1.6]), grid("KA", [18.76, 0.0], [-5.21, 0.0]),
                         grid("SG", [6.0, 18.59], [-84.0, 18.59]), grid("16", [16.26, 21.09], [16.26, -1.6])];
        // the architect's: the same grids 77.64 m and 11.66 m off, given by a point only, one of
        // them (K1) moved, and one the other has not
        let architect = [grid("S1", [0.2, 0.0], [0.2, 0.0]), grid("K1", [74.6, 0.0], [74.6, 0.0]), grid("KA", [0.0, 11.66], [0.0, 11.66]),
                         grid("SG", [0.0, 30.25], [0.0, 30.25]), grid("Z1", [5.0, 5.0], [9.0, 5.0])];
        let aligned = on_grids(&structure, &architect).expect("aligned");
        assert!((aligned.by - DVec3::new(-77.64, -11.66, 0.0)).length() < 1e-4, "{aligned:?}");
        assert_eq!((aligned.grids, aligned.left_out), (3, 1), "K1 does not agree and counts not");
        // the other way round, the opposite move
        let back = on_grids(&architect, &structure).expect("aligned back");
        assert!((back.by + aligned.by).length() < 1e-4, "{back:?}");
    }

    #[test]
    fn models_are_left_where_they_are_without_crossing_grids_or_when_turned() {
        let one = [grid("A", [0.0, 0.0], [10.0, 0.0]), grid("B", [0.0, 5.0], [10.0, 5.0])];
        let moved = [grid("A", [3.0, 2.0], [13.0, 2.0]), grid("B", [3.0, 7.0], [13.0, 7.0])];
        assert_eq!(on_grids(&one, &moved), None, "parallel grids alone do not fix a move along them");
        let crossing = [grid("A", [0.0, 0.0], [10.0, 0.0]), grid("1", [0.0, 0.0], [0.0, 10.0])];
        let turned = [grid("A", [0.0, 0.0], [10.0, 1.0]), grid("1", [0.0, 0.0], [-1.0, 10.0])];
        assert_eq!(on_grids(&crossing, &turned), None);
        let no_names = [grid("X", [0.0, 0.0], [10.0, 0.0]), grid("Y", [0.0, 0.0], [0.0, 10.0])];
        assert_eq!(on_grids(&crossing, &no_names), None);
        // already on each other: no move
        assert_eq!(on_grids(&crossing, &crossing).map(|a| a.by), Some(DVec3::ZERO));
    }
}
