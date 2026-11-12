//! Drawing less of a large model: only what is in view, and of that only what shows.
//!
//! A layer's triangles go to the GPU in another order than the model has them: by where their
//! element lies (a grid of cells over the model, a chunk per cell), in each chunk the largest
//! elements first, the opaque triangles apart from the transparent ones. A frame then draws,
//! per chunk, nothing when its box is out of view. While the view moves it draws only the start
//! of a chunk's triangles: up to the first element that would be smaller on screen than a pixel
//! or so at the chunk's nearest point. So a town hall turned round leaves out its bolts and
//! brackets until it stands still, and the same model seen from inside a room leaves out the
//! rest of the building.
//!
//! The model itself is not touched: picking, edges and cut faces work from its own order.

use crate::model::Layer;
use glam::{Mat4, Vec3, Vec4};
use std::collections::BTreeMap;

/// An element narrower than this on screen (pixels) is left out while the view moves.
pub const SMALLEST: f32 = 2.0;
/// How many cells the grid has along the model's longest side.
pub const CELLS: f32 = 16.0;
/// Triangles that belong to no element are taken this many indices at a time.
const RUN: usize = 30_000;

/// The triangles of the elements of one cell, opaque or transparent.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub min: Vec3,
    pub max: Vec3,
    /// Its first index in the layer's reordered indices.
    pub start: u32,
    /// Per element, the largest first: its radius, and the indices up to and with it.
    pub steps: Vec<(f32, u32)>,
    pub glass: bool,
}

/// A layer's triangles as the GPU gets them.
#[derive(Debug, Default)]
pub struct Chunked {
    pub indices: Vec<u32>,
    pub chunks: Vec<Chunk>,
}

/// What a frame sees: for leaving chunks and small elements out.
#[derive(Debug, Clone, Copy)]
pub struct Sight {
    /// Model to clip space.
    pub clip: Mat4,
    /// The eye, in the model's coordinates.
    pub eye: Vec3,
    /// Pixels a length of 1 takes at distance 1 (or anywhere, with a parallel projection).
    pub scale: f32,
    pub parallel: bool,
    /// Draw everything (the exploded view moves elements out of their chunks).
    pub all: bool,
    /// The camera stands still: every element in view is drawn, however small. A field of tiles
    /// each smaller than a pixel is a surface all the same, so the small is only left out while
    /// the view moves.
    pub still: bool,
}

/// Puts a layer's triangles in chunks. `origin` and `cell` are the grid of the whole model.
pub fn chunked(layer: &Layer, origin: Vec3, cell: f32) -> Chunked {
    let point = |i: u32| {
        let k = i as usize * 3;
        layer.positions.get(k..k + 3).map_or(Vec3::ZERO, |p| Vec3::new(p[0], p[1], p[2]))
    };
    let glassy = |i: u32| layer.colors.as_ref().and_then(|c| c.get(i as usize)).is_some_and(|c| c[3] < 253);
    // the elements' ranges, and what lies between them in runs
    let mut ranges: Vec<(usize, usize, bool)> = Vec::new();
    let mut at = 0usize;
    let loose = |from: usize, to: usize, ranges: &mut Vec<(usize, usize, bool)>| {
        let mut from = from;
        while from < to {
            let end = (from + RUN).min(to);
            ranges.push((from, end, false));
            from = end;
        }
    };
    for &(_, first, count) in &layer.elements {
        let (first, end) = (first as usize, ((first + count) as usize).min(layer.indices.len()));
        if first > at {
            loose(at, first, &mut ranges);
        }
        if end > first.max(at) {
            ranges.push((first.max(at), end, true));
            at = end;
        }
    }
    loose(at, layer.indices.len() / 3 * 3, &mut ranges);

    struct Part {
        radius: f32,
        min: Vec3,
        max: Vec3,
        indices: Vec<u32>,
    }
    // by cell and by glass or not
    let mut cells: BTreeMap<([i32; 3], bool), Vec<Part>> = BTreeMap::new();
    for (from, to, element) in ranges {
        let slice = &layer.indices[from..to];
        let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for &i in slice {
            let p = point(i);
            min = min.min(p);
            max = max.max(p);
        }
        let radius = if element { (max - min).length() / 2.0 } else { f32::INFINITY };
        let key = (((min + max) / 2.0 - origin) / cell.max(1e-6)).floor().as_ivec3().to_array();
        let (mut solid, mut glass) = (Vec::new(), Vec::new());
        for t in slice.chunks_exact(3) {
            if glassy(t[0]) || glassy(t[1]) || glassy(t[2]) { &mut glass } else { &mut solid }.extend_from_slice(t);
        }
        for (indices, is_glass) in [(solid, false), (glass, true)] {
            if !indices.is_empty() {
                cells.entry((key, is_glass)).or_default().push(Part { radius, min, max, indices });
            }
        }
    }
    let mut out = Chunked { indices: Vec::with_capacity(layer.indices.len()), chunks: Vec::with_capacity(cells.len()) };
    for ((_, glass), mut parts) in cells {
        parts.sort_by(|a, b| b.radius.total_cmp(&a.radius));
        let start = out.indices.len() as u32;
        let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let mut steps = Vec::with_capacity(parts.len());
        for part in parts {
            min = min.min(part.min);
            max = max.max(part.max);
            out.indices.extend(part.indices);
            steps.push((part.radius, out.indices.len() as u32 - start));
        }
        out.chunks.push(Chunk { min, max, start, steps, glass });
    }
    out
}

impl Chunk {
    /// How many of its indices a frame draws: none out of view, else those of the elements
    /// large enough to show.
    pub fn shown(&self, sight: &Sight) -> u32 {
        let all = self.steps.last().map_or(0, |s| s.1);
        if sight.all {
            return all;
        }
        // out of view: every corner beyond one side of what the camera sees
        let mut beyond = [true; 6];
        for k in 0..8 {
            let corner = Vec3::new(if k & 1 == 0 { self.min.x } else { self.max.x }, if k & 2 == 0 { self.min.y } else { self.max.y }, if k & 4 == 0 { self.min.z } else { self.max.z });
            let c: Vec4 = sight.clip * corner.extend(1.0);
            let inside = [c.x >= -c.w, c.x <= c.w, c.y >= -c.w, c.y <= c.w, c.z >= 0.0, c.z <= c.w];
            for (side, ok) in beyond.iter_mut().zip(inside) {
                *side &= !ok;
            }
        }
        if beyond.contains(&true) {
            return 0;
        }
        // the smallest radius that still shows at the chunk's nearest point
        let nearest = (sight.eye.clamp(self.min, self.max) - sight.eye).length();
        if sight.still {
            return all;
        }
        let least = SMALLEST / 2.0 / sight.scale.max(1e-6) * if sight.parallel { 1.0 } else { nearest };
        let count = self.steps.partition_point(|s| s.0 >= least);
        // the largest element of a chunk in view always shows, however small
        self.steps[count.max(1).min(self.steps.len()) - 1].1
    }
}

/// The index ranges of a layer a frame draws, of its opaque or its transparent chunks; ranges
/// that follow each other are drawn as one.
pub fn draws(chunks: &[Chunk], sight: &Sight, glass: bool, layer: u32, out: &mut Vec<(u32, u32, u32)>) {
    for chunk in chunks.iter().filter(|c| c.glass == glass) {
        let count = chunk.shown(sight);
        if count == 0 {
            continue;
        }
        match out.last_mut() {
            Some((l, _, end)) if *l == layer && *end == chunk.start => *end = chunk.start + count,
            _ => out.push((layer, chunk.start, chunk.start + count)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layer of boxes: each an element of one triangle of `size` at `at`.
    fn layer(parts: &[(Vec3, f32, u8)]) -> Layer {
        let mut layer = Layer { colors: Some(Vec::new()), ..Layer::default() };
        for (k, (at, size, alpha)) in parts.iter().enumerate() {
            for corner in [Vec3::ZERO, Vec3::X * *size, Vec3::Y * *size] {
                layer.positions.extend((*at + corner).to_array());
                layer.normals.extend([0.0, 0.0, 1.0]);
                layer.colors.as_mut().unwrap().push([200, 200, 200, *alpha]);
            }
            let first = k as u32 * 3;
            layer.indices.extend([first, first + 1, first + 2]);
            layer.elements.push((k as u64 + 1, first, 3));
        }
        layer
    }

    fn sight(eye: Vec3, target: Vec3) -> Sight {
        let clip = Mat4::perspective_rh(1.0, 1.0, 0.1, 10_000.0) * Mat4::look_at_rh(eye, target, Vec3::Y);
        // 1000 pixels high, 1 radian: a length of 1 at distance 1 is some 915 pixels
        Sight { clip, eye, scale: 500.0 / (0.5f32).tan(), parallel: false, all: false, still: false }
    }

    #[test]
    fn every_triangle_is_kept_once_and_glass_lies_apart() {
        let l = layer(&[(Vec3::ZERO, 1.0, 255), (Vec3::new(50.0, 0.0, 0.0), 2.0, 255), (Vec3::new(1.0, 1.0, 0.0), 0.5, 80)]);
        let c = chunked(&l, Vec3::ZERO, 10.0);
        let mut all = c.indices.clone();
        all.sort_unstable();
        assert_eq!(all, (0..9).collect::<Vec<u32>>());
        // two cells, and the glass of the first in a chunk of its own
        assert_eq!(c.chunks.len(), 3);
        assert_eq!(c.chunks.iter().filter(|k| k.glass).map(|k| k.steps.len()).collect::<Vec<_>>(), [1]);
        // triangles that belong to no element are kept too
        let mut bare = l.clone();
        bare.elements.remove(1);
        let c = chunked(&bare, Vec3::ZERO, 10.0);
        assert_eq!(c.indices.len(), 9);
    }

    #[test]
    fn what_is_out_of_view_or_too_small_is_not_drawn() {
        // a wall and a bolt beside it, and a second wall far to the side
        let l = layer(&[(Vec3::ZERO, 3.0, 255), (Vec3::new(0.5, 0.5, 0.0), 0.01, 255), (Vec3::new(500.0, 0.0, 0.0), 3.0, 255)]);
        let c = chunked(&l, Vec3::ZERO, 10.0);
        let count = |s: &Sight| {
            let mut out = Vec::new();
            draws(&c.chunks, s, false, 0, &mut out);
            out.iter().map(|(_, from, to)| to - from).sum::<u32>()
        };
        // near: the wall and its bolt, not the wall out of view
        assert_eq!(count(&sight(Vec3::new(1.0, 1.0, 5.0), Vec3::new(1.0, 1.0, 0.0))), 6);
        // from 400 m the bolt is a fiftieth of a pixel: the wall alone
        assert_eq!(count(&sight(Vec3::new(1.0, 1.0, 400.0), Vec3::new(1.0, 1.01, 0.0))), 3);
        // standing still, the bolt is drawn again; what is out of view is not
        assert_eq!(count(&Sight { still: true, ..sight(Vec3::new(1.0, 1.0, 400.0), Vec3::new(1.0, 1.01, 0.0)) }), 6);
        // the exploded view draws everything
        assert_eq!(count(&Sight { all: true, ..sight(Vec3::new(1.0, 1.0, 400.0), Vec3::new(1.0, 1.01, 0.0)) }), 9);
        // a chunk in view shows its largest element however small it is
        let far = sight(Vec3::new(1.0, 1.0, 9000.0), Vec3::new(1.0, 1.01, 0.0));
        assert!(count(&far) >= 3);
    }
}
