//! Object File Format (.off), as Geomview, CGAL, MeshLab and the shape benchmarks write it: ASCII
//! `OFF` with its variants `COFF` (vertex colours), `NOFF` (normals), `STOFF` (texture
//! coordinates) and per-face colours. One layer, flat shaded, polygons fan triangulated; kept
//! as it is (no axis or unit change). Binary OFF and the 4D variants are not read.

use super::{Loader, Source, DEFAULT_COLOR};
use crate::model::Layer;
use glam::Vec3;

pub struct OffLoader;

impl Loader for OffLoader {
    fn name(&self) -> &'static str {
        "OFF"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["off"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let data = source.read()?;
        let mut layer = read(&String::from_utf8_lossy(&data))?;
        layer.name = source.stem();
        Ok(vec![layer])
    }
}

/// A colour of 3 or 4 values: 0..1 floats or 0..255 integers.
fn color(values: &[&str]) -> Option<[u8; 4]> {
    if !(3..=4).contains(&values.len()) {
        return None;
    }
    let numbers: Vec<f32> = values.iter().map(|v| v.parse().ok()).collect::<Option<_>>()?;
    let integers = values.iter().all(|v| !v.contains('.')) && numbers.iter().any(|&v| v > 1.0);
    let scale = if integers { 1.0 } else { 255.0 };
    let c = |i: usize| numbers.get(i).map_or(255, |v| (v * scale).round().clamp(0.0, 255.0) as u8);
    Some([c(0), c(1), c(2), 255])
}

fn read(text: &str) -> Result<Layer, String> {
    // the words of the file without comments; the header may share its line with the counts
    let mut lines = text.lines().map(|l| l.split('#').next().unwrap_or("").trim()).filter(|l| !l.is_empty());
    let first = lines.next().ok_or("OFF: empty file")?;
    let mut words = first.split_whitespace();
    let header = words.next().unwrap_or("");
    let Some(prefix) = header.strip_suffix("OFF") else { return Err("OFF: no OFF header".into()) };
    if prefix.contains('4') || prefix.contains('n') || words.clone().next() == Some("BINARY") {
        return Err("OFF: binary and higher-dimensional OFF files are not supported".into());
    }
    let counts_line: Vec<&str> = match words.next() {
        Some(w) => std::iter::once(w).chain(words).collect(),
        None => lines.next().ok_or("OFF: no counts")?.split_whitespace().collect(),
    };
    let count = |i: usize| counts_line.get(i).and_then(|c| c.parse::<usize>().ok()).ok_or("OFF: bad vertex/face counts");
    let (vertex_count, face_count) = (count(0)?, count(1)?);
    // vertex line: x y z [nx ny nz] [r g b a] [s t]
    let normal_values = if prefix.contains('N') { 3 } else { 0 };
    let vertex_colors = prefix.contains('C');
    let mut points = Vec::with_capacity(vertex_count.min(1 << 24));
    let mut point_colors = Vec::new();
    for _ in 0..vertex_count {
        let v: Vec<&str> = lines.next().ok_or("OFF: fewer vertices than its count")?.split_whitespace().collect();
        let xyz: Vec<f32> = v.iter().take(3).map(|s| s.parse().map_err(|_| format!("OFF: bad vertex {s:?}"))).collect::<Result<_, _>>()?;
        if xyz.len() < 3 {
            return Err("OFF: vertex with fewer than 3 coordinates".into());
        }
        points.push(Vec3::new(xyz[0], xyz[1], xyz[2]));
        if vertex_colors {
            let rest = v.get(3 + normal_values..).unwrap_or(&[]);
            point_colors.push(color(&rest[..rest.len().min(4)]).or_else(|| color(&rest[..rest.len().min(3)])));
        }
    }
    let mut layer = Layer { color: DEFAULT_COLOR, ..Layer::default() };
    let mut colors = Vec::new();
    let mut any_color = false;
    let default = DEFAULT_COLOR.map(|v| (v * 255.0).round() as u8);
    let default = [default[0], default[1], default[2], 255];
    for _ in 0..face_count {
        let Some(line) = lines.next() else { break };
        let f: Vec<&str> = line.split_whitespace().collect();
        let n = f.first().and_then(|n| n.parse::<usize>().ok()).ok_or("OFF: bad face line")?;
        let Some(corners) = f.get(1..1 + n).map(|c| c.iter().filter_map(|i| i.parse::<usize>().ok()).collect::<Vec<_>>()) else { continue };
        if corners.len() < 3 || corners.iter().any(|&i| i >= points.len()) {
            continue;
        }
        let face_color = color(f.get(1 + n..).unwrap_or(&[]));
        let p: Vec<Vec3> = corners.iter().map(|&i| points[i]).collect();
        // Newell normal of the polygon
        let mut normal = Vec3::ZERO;
        for i in 0..p.len() {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            normal += Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
        }
        let normal = normal.normalize_or_zero();
        for k in 1..p.len() - 1 {
            for i in [0, k, k + 1] {
                let c = face_color.or_else(|| point_colors.get(corners[i]).copied().flatten());
                any_color |= c.is_some();
                colors.push(c.unwrap_or(default));
                layer.indices.push(layer.vertex_count() as u32);
                layer.positions.extend_from_slice(&p[i].to_array());
                layer.normals.extend_from_slice(&normal.to_array());
            }
        }
    }
    if layer.indices.is_empty() {
        return Err("OFF: no faces".into());
    }
    if any_color {
        layer.colors = Some(colors);
    }
    Ok(layer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn reads_coff_with_face_colours_and_comments() {
        // a unit square (quad) and a triangle with its own colour; vertex colours 0..1
        let text = "COFF # coloured\n# counts\n5 2 0\n0 0 0 1 0 0 1\n1 0 0 1 0 0 1\n1 1 0 1 0 0 1\n0 1 0 1 0 0 1\n0 0 1 0 0 1 1\n4 0 1 2 3\n3 0 1 4 0 255 0\n";
        let source = Source::Bytes { name: "mesh.OFF".into(), data: Arc::new(text.as_bytes().to_vec()) };
        let model = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("OFF");
        assert_eq!(model.format, "OFF");
        assert_eq!(model.triangle_count(), 3);
        assert_eq!(model.bounds.max, [1.0, 1.0, 1.0]);
        let layer = &model.layers[0];
        assert_eq!(layer.name, "mesh");
        let colors = layer.colors.as_ref().unwrap();
        assert_eq!(colors[0], [255, 0, 0, 255]);
        assert_eq!(colors[6], [0, 255, 0, 255]);
        assert!(layer.normals[..18].chunks_exact(3).all(|n| n == [0.0, 0.0, 1.0]));
    }

    #[test]
    fn header_and_counts_on_one_line() {
        let layer = read("OFF 3 1 0\n0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n").unwrap();
        assert_eq!(layer.triangle_count(), 1);
        assert!(layer.colors.is_none());
        assert!(read("PLY\n").is_err());
    }
}
