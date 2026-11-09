//! Tile frames: a fixed square grid of local coordinate frames over the project (ADR-0006 and
//! ADR-0016 of Open BIM Studio).
//!
//! A tile is a coordinate frame and a unit of loading, never a semantic boundary. Every element
//! belongs to the tile that holds its placement origin, and its coordinates are f64 metres in that
//! tile's frame; its geometry may reach into neighbouring tiles, and it is never split.
//!
//! The grid is anchored at the project origin and its size is a whole number of metres, so every
//! tile origin is a whole number of metres. Whole numbers are exact in f64 up to 2⁵³, so moving a
//! point from one tile frame to another adds an exact difference: the only rounding is the one sum
//! itself. Heights are not tiled: a tile is a column over a square of the plan.

use crate::Refusal;
use glam::DVec3;
use serde::{Deserialize, Serialize};

/// A tile of the grid: its column (`x`) and row (`y`), counted from the project origin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TileKey {
    pub x: i32,
    pub y: i32,
}

/// A point in a tile frame: the tile and the f64 coordinates (metres) from its origin.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TilePoint {
    pub tile: TileKey,
    pub local: DVec3,
}

/// The project's tile grid: square tiles of a whole number of metres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileGrid {
    size_m: u32,
}

impl Default for TileGrid {
    /// Tiles of 1 km.
    fn default() -> Self {
        Self { size_m: 1000 }
    }
}

impl TileGrid {
    /// The largest tile, 100 km.
    pub const MAX_SIZE_M: u32 = 100_000;

    /// A grid of tiles `size_m` metres wide (1 m to 100 km).
    pub fn new(size_m: u32) -> Result<Self, Refusal> {
        if (1..=Self::MAX_SIZE_M).contains(&size_m) {
            Ok(Self { size_m })
        } else {
            Err(Refusal::BadTileSize)
        }
    }

    /// The width of a tile in metres.
    pub fn size(&self) -> f64 {
        f64::from(self.size_m)
    }

    /// The origin of tile `key` in the project frame: whole metres, so exact.
    pub fn origin(&self, key: TileKey) -> DVec3 {
        let size = i64::from(self.size_m);
        DVec3::new((i64::from(key.x) * size) as f64, (i64::from(key.y) * size) as f64, 0.0)
    }

    /// The tile that holds project point `p` (a point on a boundary belongs to the tile after it).
    pub fn tile_of(&self, p: DVec3) -> TileKey {
        let index = |v: f64| (v / self.size()).floor().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
        TileKey { x: index(p.x), y: index(p.y) }
    }

    /// Project point `p` in the frame of the tile that holds it.
    pub fn to_local(&self, p: DVec3) -> TilePoint {
        let tile = self.tile_of(p);
        TilePoint { tile, local: p - self.origin(tile) }
    }

    /// Project point `p` in the frame of tile `tile` (which need not hold it).
    pub fn local_in(&self, p: DVec3, tile: TileKey) -> DVec3 {
        p - self.origin(tile)
    }

    /// A tile point in the project frame.
    pub fn to_project(&self, p: TilePoint) -> DVec3 {
        self.origin(p.tile) + p.local
    }

    /// The offset that takes coordinates of tile `from` to tile `to`: whole metres, so exact.
    pub fn offset(&self, from: TileKey, to: TileKey) -> DVec3 {
        let size = i64::from(self.size_m);
        let d = |a: i32, b: i32| ((i64::from(a) - i64::from(b)) * size) as f64;
        DVec3::new(d(from.x, to.x), d(from.y, to.y), 0.0)
    }

    /// `local`, a point of tile `from`, in the frame of tile `to`.
    pub fn transfer(&self, local: DVec3, from: TileKey, to: TileKey) -> DVec3 {
        local + self.offset(from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_belongs_to_the_tile_that_holds_it() {
        let grid = TileGrid::default();
        assert_eq!(grid.tile_of(DVec3::new(0.0, 0.0, 0.0)), TileKey { x: 0, y: 0 });
        assert_eq!(grid.tile_of(DVec3::new(999.999, 1000.0, 50.0)), TileKey { x: 0, y: 1 });
        assert_eq!(grid.tile_of(DVec3::new(-0.001, -1000.0, 0.0)), TileKey { x: -1, y: -1 });
    }

    #[test]
    fn far_from_the_origin_local_coordinates_stay_small_and_exact() {
        // 500 km out, as Open BIM Studio's precision set
        let grid = TileGrid::default();
        let p = DVec3::new(500_000.123_456, 300_000.654_321, 12.5);
        let t = grid.to_local(p);
        assert_eq!(t.tile, TileKey { x: 500, y: 300 });
        assert!(t.local.x < 1.0 && t.local.y < 1.0);
        assert_eq!(grid.to_project(t), p, "the tile origin is whole metres: the sum gives p back exactly");
    }

    #[test]
    fn the_offset_between_tiles_is_exact_and_a_transfer_rounds_once() {
        let grid = TileGrid::new(250).unwrap();
        let (a, b) = (TileKey { x: 2000, y: -7 }, TileKey { x: 1999, y: -6 });
        assert_eq!(grid.offset(a, b), DVec3::new(250.0, -250.0, 0.0));
        assert_eq!(grid.offset(a, b), grid.origin(a) - grid.origin(b), "whole metres: no rounding");
        let local = DVec3::new(0.1, 0.2, 0.3);
        let there = grid.transfer(local, a, b);
        assert_eq!(there, DVec3::new(250.1, -249.8, 0.3), "one correctly rounded sum");
        // back again rounds a second time, at the magnitude of 250 m: within its last place, not
        // the identity; which is why an element keeps its own tile frame and is only carried over
        // when it moves home
        let back = grid.transfer(there, b, a);
        assert!((back - local).abs().max_element() <= 250f64 * f64::EPSILON, "{back}");
    }

    #[test]
    fn tile_sizes_are_whole_metres_within_bounds() {
        assert_eq!(TileGrid::new(0), Err(Refusal::BadTileSize));
        assert_eq!(TileGrid::new(TileGrid::MAX_SIZE_M + 1), Err(Refusal::BadTileSize));
        assert_eq!(TileGrid::new(1).unwrap().size(), 1.0);
    }
}
