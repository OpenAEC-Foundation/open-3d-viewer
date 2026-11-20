//! glTF models placed by a matrix and gathered into layers, for the formats whose reader hands
//! over glTF with placements (3D Tiles, Voyager scenes).

use super::{Loader, Source};
use crate::model::Layer;
use std::collections::BTreeMap;
use std::sync::Arc;

/// A glTF binary held in memory, as a source for the glTF reader.
pub fn glb(bytes: Vec<u8>) -> Source {
    Source::Bytes { name: "content.glb".into(), data: Arc::new(bytes) }
}

/// Layers gathered by name over many placed models; element ids kept apart per placement.
#[derive(Default)]
pub struct Gather {
    pub layers: BTreeMap<String, Layer>,
    next_element: u64,
}

impl Gather {
    /// Reads a glTF with the app's glTF reader (Z up) and adds it once per placement
    /// (column-major 4x4, from the glTF's Z-up frame to the model's); each element gets the
    /// extra properties.
    pub fn gltf(&mut self, source: &Source, placements: &[[f64; 16]], extra: &[(&str, String)]) -> Result<(), String> {
        let layers = super::gltf::GltfLoader.load(source)?;
        for placement in placements {
            for layer in &layers {
                self.add(placed(layer, placement), extra);
            }
            self.next_element += layers.iter().flat_map(|l| l.elements.iter().map(|e| e.0)).max().unwrap_or(0) + 1;
        }
        Ok(())
    }

    /// Adds a layer under its name, its element ids moved past the ones before.
    pub fn add(&mut self, mut layer: Layer, extra: &[(&str, String)]) {
        let base = self.next_element;
        for e in &mut layer.elements {
            e.0 += base;
        }
        for (id, props) in &mut layer.props {
            *id += base;
            if !extra.is_empty() {
                if let Ok(serde_json::Value::Object(mut map)) = serde_json::from_str::<serde_json::Value>(props) {
                    for (k, v) in extra {
                        map.insert((*k).to_owned(), v.clone().into());
                    }
                    *props = serde_json::Value::Object(map).to_string();
                }
            }
        }
        match self.layers.get_mut(&layer.name) {
            Some(into) => append(into, layer),
            None => {
                self.layers.insert(layer.name.clone(), layer);
            }
        }
    }

    pub fn into_layers(self) -> Vec<Layer> {
        self.layers.into_values().collect()
    }
}

/// A copy of a layer moved by a column-major matrix (normals turned with it).
pub fn placed(layer: &Layer, m: &[f64; 16]) -> Layer {
    let mut out = layer.clone();
    for p in out.positions.chunks_exact_mut(3) {
        let (x, y, z) = (p[0] as f64, p[1] as f64, p[2] as f64);
        for r in 0..3 {
            p[r] = (m[r] * x + m[4 + r] * y + m[8 + r] * z + m[12 + r]) as f32;
        }
    }
    // normals: the inverse transpose of the 3x3 part
    let a = |c: usize, r: usize| m[c * 4 + r];
    let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(2, 1) * a(1, 2)) - a(1, 0) * (a(0, 1) * a(2, 2) - a(2, 1) * a(0, 2)) + a(2, 0) * (a(0, 1) * a(1, 2) - a(1, 1) * a(0, 2));
    if det.abs() > 1e-30 {
        // cofactor matrix = det * inverse transpose
        let cof = |r: usize, c: usize| {
            let (r1, r2, c1, c2) = ((r + 1) % 3, (r + 2) % 3, (c + 1) % 3, (c + 2) % 3);
            a(c1, r1) * a(c2, r2) - a(c2, r1) * a(c1, r2)
        };
        for n in out.normals.chunks_exact_mut(3) {
            let v = [n[0] as f64, n[1] as f64, n[2] as f64];
            let mut w = [0.0; 3];
            for r in 0..3 {
                w[r] = (0..3).map(|c| cof(r, c) * v[c]).sum::<f64>() * det.signum();
            }
            let len = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt().max(1e-30);
            for r in 0..3 {
                n[r] = (w[r] / len) as f32;
            }
        }
        if det < 0.0 {
            // a mirror turns the triangles inside out
            for t in out.indices.chunks_exact_mut(3) {
                t.swap(1, 2);
            }
        }
    }
    out
}

/// Appends a layer's geometry to another of the same name.
pub fn append(into: &mut Layer, layer: Layer) {
    let offset = (into.positions.len() / 3) as u32;
    let first = into.indices.len() as u32;
    let first_line = into.lines.len() as u32;
    let new_vertices = layer.positions.len() / 3;
    let fill = into.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let fill = [fill[0], fill[1], fill[2], 255];
    match (&mut into.colors, layer.colors) {
        (Some(c), Some(n)) => c.extend(n),
        (Some(c), None) => c.extend(std::iter::repeat_n(fill, new_vertices)),
        (None, Some(n)) => {
            let mut all = vec![fill; offset as usize];
            all.extend(n);
            into.colors = Some(all);
        }
        (None, None) => {}
    }
    into.positions.extend(layer.positions);
    into.normals.extend(layer.normals);
    into.indices.extend(layer.indices.into_iter().map(|i| i + offset));
    into.lines.extend(layer.lines.into_iter().map(|i| i + offset));
    into.elements.extend(layer.elements.into_iter().map(|(id, f, n)| (id, f + first, n)));
    into.line_elements.extend(layer.line_elements.into_iter().map(|(id, f, n)| (id, f + first_line, n)));
    into.props.extend(layer.props);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placing_moves_points_and_turns_normals() {
        let layer = Layer { name: "a".into(), positions: vec![1.0, 0.0, 0.0], normals: vec![1.0, 0.0, 0.0], ..Layer::default() };
        // a quarter turn about Z and a move of 10 along X
        let m = [0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 10.0, 0.0, 0.0, 1.0];
        let p = placed(&layer, &m);
        assert_eq!(p.positions, vec![10.0, 1.0, 0.0]);
        assert!((p.normals[1] - 1.0).abs() < 1e-6 && p.normals[0].abs() < 1e-6);
        let mut g = Gather::default();
        g.add(p.clone(), &[]);
        g.add(p, &[]);
        assert_eq!(g.into_layers()[0].positions.len(), 6);
    }
}
