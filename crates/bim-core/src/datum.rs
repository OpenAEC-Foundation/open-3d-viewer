//! Grid lines: the straight lines a project is set out on. A grid line is a datum, not an
//! element: it has a name and two ends in plan (project coordinates, metres) and no body.
//!
//! A new grid line names itself. A line that runs more north-south than east-west is numbered
//! (1, 2, 3 …); any other is lettered (A, B … Z, then A2, B2 …). The name is the one after the
//! last of its kind in use, so a name given away is not given again while a later one stands.

use crate::model::Model;
use crate::{Refusal, LINEAR_TOLERANCE};
use glam::DVec2;
use serde::{Deserialize, Serialize};

/// A grid line: its name and its ends in plan, in project coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridDef {
    pub name: String,
    pub start: DVec2,
    pub end: DVec2,
}

impl GridDef {
    /// Whether it runs more north-south than east-west: such lines are numbered.
    pub fn numbered(start: DVec2, end: DVec2) -> bool {
        let d = end - start;
        d.y.abs() > d.x.abs()
    }

    /// Where it crosses `other`, within the length of both.
    pub fn crossing(&self, other: &GridDef) -> Option<DVec2> {
        let (r, s) = (self.end - self.start, other.end - other.start);
        let denom = r.perp_dot(s);
        if denom.abs() < 1e-12 * r.length() * s.length() {
            return None;
        }
        let d = other.start - self.start;
        let (t, u) = (d.perp_dot(s) / denom, d.perp_dot(r) / denom);
        ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| self.start + r * t)
    }
}

/// The place of a lettered name in A, B … Z, A2, B2 …; `None` for any other name.
fn letter_index(name: &str) -> Option<u64> {
    let mut chars = name.chars();
    let letter = chars.next().filter(char::is_ascii_uppercase)?;
    let round = match chars.as_str() {
        "" => 1,
        digits => digits.parse::<u64>().ok().filter(|r| *r >= 2)?,
    };
    Some((round - 1) * 26 + (letter as u64 - 'A' as u64))
}

/// The lettered name at place `index` of A, B … Z, A2, B2 …
fn letter_name(index: u64) -> String {
    let letter = (b'A' + (index % 26) as u8) as char;
    match index / 26 {
        0 => letter.to_string(),
        round => format!("{letter}{}", round + 1),
    }
}

impl Model {
    /// The grid lines, in the order they were made.
    pub fn grids(&self) -> &[GridDef] {
        &self.grids
    }

    /// The name a new grid line from `start` to `end` gets: the number after the highest in use
    /// for one that runs north-south, else the letter after the last in use.
    pub fn next_grid_name(&self, start: DVec2, end: DVec2) -> String {
        if GridDef::numbered(start, end) {
            let last = self.grids.iter().filter_map(|g| g.name.parse::<u64>().ok()).max().unwrap_or(0);
            return (last + 1).to_string();
        }
        let next = self.grids.iter().filter_map(|g| letter_index(&g.name)).max().map_or(0, |i| i + 1);
        letter_name(next)
    }

    /// Adds a grid line from `start` to `end` under the next name of its kind; returns its
    /// index. A line without length is refused.
    pub fn add_grid(&mut self, start: DVec2, end: DVec2) -> Result<usize, Refusal> {
        let name = self.next_grid_name(start, end);
        self.insert_grid(GridDef { name, start, end })
    }

    /// Adds grid line `grid` as it is (reading a file); returns its index. A line without length
    /// is refused.
    pub fn insert_grid(&mut self, grid: GridDef) -> Result<usize, Refusal> {
        if (grid.end - grid.start).length() <= LINEAR_TOLERANCE || !grid.start.is_finite() || !grid.end.is_finite() {
            return Err(Refusal::ZeroLength);
        }
        self.grids.push(grid);
        Ok(self.grids.len() - 1)
    }

    /// Changes grid line `index`; a line without length, or an index out of range, is refused.
    pub fn set_grid(&mut self, index: usize, grid: GridDef) -> Result<(), Refusal> {
        if (grid.end - grid.start).length() <= LINEAR_TOLERANCE || !grid.start.is_finite() || !grid.end.is_finite() {
            return Err(Refusal::ZeroLength);
        }
        *self.grids.get_mut(index).ok_or(Refusal::Missing)? = grid;
        Ok(())
    }

    /// Removes grid line `index`; returns it.
    pub fn remove_grid(&mut self, index: usize) -> Option<GridDef> {
        (index < self.grids.len()).then(|| self.grids.remove(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile::TileGrid;

    fn p(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn a_new_grid_line_names_itself_by_the_way_it_runs() {
        let mut m = Model::new(TileGrid::default());
        // north-south: numbered; east-west and diagonal: lettered
        let names = |m: &Model| m.grids().iter().map(|g| g.name.clone()).collect::<Vec<_>>();
        m.add_grid(p(0.0, 0.0), p(0.0, 10.0)).unwrap();
        m.add_grid(p(5.0, 10.0), p(5.0, 0.0)).unwrap();
        m.add_grid(p(0.0, 0.0), p(10.0, 0.0)).unwrap();
        m.add_grid(p(0.0, 4.0), p(10.0, 4.0)).unwrap();
        m.add_grid(p(0.0, 0.0), p(10.0, 10.0)).unwrap();
        assert_eq!(names(&m), ["1", "2", "A", "B", "C"]);
        // a name given away is not given again while a later one stands
        m.remove_grid(0).unwrap();
        m.remove_grid(1).unwrap();
        m.add_grid(p(9.0, 0.0), p(9.0, 10.0)).unwrap();
        m.add_grid(p(0.0, 9.0), p(10.0, 9.0)).unwrap();
        assert_eq!(names(&m), ["2", "B", "C", "3", "D"]);
        // after Z comes A2
        m.insert_grid(GridDef { name: "Z".into(), start: p(0.0, 20.0), end: p(10.0, 20.0) }).unwrap();
        assert_eq!(m.next_grid_name(p(0.0, 0.0), p(1.0, 0.0)), "A2");
        m.insert_grid(GridDef { name: "Z2".into(), start: p(0.0, 21.0), end: p(10.0, 21.0) }).unwrap();
        assert_eq!(m.next_grid_name(p(0.0, 0.0), p(1.0, 0.0)), "A3");
        // names of one's own do not count
        m.insert_grid(GridDef { name: "Gevel".into(), start: p(0.0, 22.0), end: p(10.0, 22.0) }).unwrap();
        assert_eq!(m.next_grid_name(p(0.0, 0.0), p(1.0, 0.0)), "A3");
    }

    #[test]
    fn a_grid_line_needs_a_length_and_crosses_others_within_both() {
        let mut m = Model::new(TileGrid::default());
        assert_eq!(m.add_grid(p(1.0, 1.0), p(1.0, 1.0)), Err(Refusal::ZeroLength));
        let one = m.add_grid(p(2.0, -1.0), p(2.0, 6.0)).unwrap();
        let a = m.add_grid(p(-1.0, 3.0), p(8.0, 3.0)).unwrap();
        let short = m.add_grid(p(4.0, 5.0), p(8.0, 5.0)).unwrap();
        let g = m.grids();
        assert_eq!(g[one].crossing(&g[a]), Some(p(2.0, 3.0)));
        assert_eq!(g[one].crossing(&g[short]), None, "it ends before the other");
        assert_eq!(g[a].crossing(&g[short]), None, "parallel");
        // moved, renamed, removed
        m.set_grid(one, GridDef { name: "X".into(), start: p(3.0, -1.0), end: p(3.0, 6.0) }).unwrap();
        assert_eq!(m.grids()[one].name, "X");
        assert_eq!(m.set_grid(9, GridDef { name: "Y".into(), start: p(0.0, 0.0), end: p(1.0, 0.0) }), Err(Refusal::Missing));
        assert_eq!(m.remove_grid(one).map(|g| g.name), Some("X".into()));
        assert_eq!(m.grids().len(), 2);
    }

    #[test]
    fn a_model_keeps_its_grid_lines_when_written_and_read() {
        let mut m = Model::new(TileGrid::default());
        m.add_grid(p(0.0, 0.0), p(0.0, 10.0)).unwrap();
        let read: Model = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(read, m);
        // a model written before grid lines were kept has none
        let old: Model = serde_json::from_str(&serde_json::to_string(&Model::new(TileGrid::default())).unwrap().replace(",\"grids\":[]", "")).unwrap();
        assert!(old.grids().is_empty());
    }
}
