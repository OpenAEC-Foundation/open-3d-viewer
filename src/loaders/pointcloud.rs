//! Shared by the point cloud readers (LAS/LAZ, E57): points into point layers, thinned out
//! evenly to at most [`MAX_POINTS`], and moved to a local origin when the coordinates are far
//! from zero (survey coordinates do not fit an f32 to the millimetre). The origin is kept in the
//! project information, as the CityJSON and LandXML readers do.

use crate::model::Layer;
use std::collections::HashMap;

/// At most this many points are drawn; a larger cloud keeps every n-th point. The browser has
/// less memory (32-bit wasm) and WebGL2 smaller buffers.
pub const MAX_POINTS: usize = if cfg!(target_arch = "wasm32") { 4_000_000 } else { 10_000_000 };

/// Coordinates beyond this (metres, or file units) move to a local origin; nearer ones stay, so a
/// scan in its own frame lines up with a model added next to it.
const FAR: f64 = 10_000.0;

/// Points read so far, per layer.
pub struct Cloud {
    stride: u64,
    seen: u64,
    origin: Option<[f64; 3]>,
    layers: Vec<Layer>,
    keys: HashMap<u32, usize>,
}

impl Cloud {
    /// For a file of `total` points, of which at most `max` are kept.
    pub fn new(total: u64, max: usize) -> Self {
        let stride = total.div_ceil(max.max(1) as u64).max(1);
        Self { stride, seen: 0, origin: None, layers: Vec::new(), keys: HashMap::new() }
    }

    /// Takes the local origin from `min`, the low corner of the cloud as the file tells it,
    /// rather than from the first point.
    pub fn anchor(&mut self, min: [f64; 3]) {
        if min.iter().all(|v| v.is_finite()) {
            self.origin = Some(origin_for(min));
        }
    }

    /// Whether the next point of the file is kept: called once per point, in file order.
    pub fn keep(&mut self) -> bool {
        let keep = self.seen % self.stride == 0;
        self.seen += 1;
        keep
    }

    /// The layer for `key`, made by `make` the first time.
    pub fn layer(&mut self, key: u32, make: impl FnOnce() -> Layer) -> usize {
        let layers = &mut self.layers;
        *self.keys.entry(key).or_insert_with(|| {
            layers.push(Layer { points: true, colors: Some(Vec::new()), ..make() });
            layers.len() - 1
        })
    }

    /// Adds a point (file coordinates) with its colour to layer `layer`.
    pub fn push(&mut self, layer: usize, p: [f64; 3], color: [u8; 3]) {
        if !p.iter().all(|v| v.is_finite()) {
            return;
        }
        let origin = *self.origin.get_or_insert_with(|| origin_for(p));
        let layer = &mut self.layers[layer];
        layer.positions.extend((0..3).map(|k| (p[k] - origin[k]) as f32));
        if let Some(colors) = layer.colors.as_mut() {
            colors.push([color[0], color[1], color[2], 255]);
        }
    }

    /// Points kept so far.
    pub fn kept(&self) -> usize {
        self.layers.iter().map(Layer::vertex_count).sum()
    }

    /// Project information: the origin taken off (when moved), the points in the file and drawn.
    pub fn info(&self, format: &str, total: u64) -> Vec<(String, String)> {
        let mut info = Vec::new();
        if let Some(o) = self.origin.filter(|o| *o != [0.0; 3]) {
            info.push((format!("{format} world origin (source coordinates)"), format!("{}, {}, {}", o[0], o[1], o[2])));
        }
        info.push(("Points in file".into(), total.to_string()));
        if self.stride > 1 {
            info.push(("Points shown".into(), format!("{} (1 in {})", self.kept(), self.stride)));
        }
        info
    }

    /// The layers, in the order of `key` when `sort`, else in the order first seen.
    pub fn into_layers(mut self, sort: bool) -> Vec<Layer> {
        if sort {
            let mut keyed: Vec<(u32, usize)> = self.keys.into_iter().collect();
            keyed.sort();
            let mut slots: Vec<Option<Layer>> = self.layers.into_iter().map(Some).collect();
            return keyed.into_iter().filter_map(|(_, i)| slots[i].take()).collect();
        }
        std::mem::take(&mut self.layers)
    }
}

/// The local origin for a cloud near `p`: `p` in whole units when it is far from zero.
fn origin_for(p: [f64; 3]) -> [f64; 3] {
    match p.iter().any(|v| v.abs() > FAR) {
        true => p.map(f64::floor),
        false => [0.0; 3],
    }
}

/// The world origin of a cloud as a model would have it (for tests).
#[cfg(test)]
pub fn origin_of(info: &[(String, String)]) -> Option<&str> {
    info.iter().find(|(k, _)| k.contains("origin")).map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thins_out_evenly_and_moves_far_points() {
        let mut cloud = Cloud::new(10, 4);
        let mut kept = 0;
        for i in 0..10 {
            if cloud.keep() {
                let l = cloud.layer(i % 2, || Layer { name: format!("L{}", i % 2), ..Layer::default() });
                cloud.push(l, [155_000.25 + i as f64, 463_000.5, 2.0], [1, 2, 3]);
                kept += 1;
            }
        }
        // every third point: 0, 3, 6, 9
        assert_eq!((kept, cloud.kept()), (4, 4));
        let info = cloud.info("LAS", 10);
        assert_eq!(origin_of(&info), Some("155000, 463000, 2"));
        assert!(info.iter().any(|(k, v)| k == "Points shown" && v.starts_with("4 ")));
        let layers = cloud.into_layers(true);
        assert_eq!(layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["L0", "L1"]);
        assert!(layers.iter().all(|l| l.points && l.colors.as_ref().is_some_and(|c| c.len() == l.vertex_count())));
        // point 0 (layer 0): 0.25, 0.5, 0
        assert_eq!(&layers[0].positions[..3], &[0.25, 0.5, 0.0]);
        // near points stay where they are
        let mut near = Cloud::new(1, 4);
        assert!(near.keep());
        let l = near.layer(0, Layer::default);
        near.push(l, [1.0, 2.0, 3.0], [0; 3]);
        assert_eq!(origin_of(&near.info("E57", 1)), None);
        assert_eq!(near.into_layers(false)[0].positions, [1.0, 2.0, 3.0]);
    }
}
