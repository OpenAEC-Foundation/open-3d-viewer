//! STL, flat shaded and kept Z up. Binary files go through `stl_io`; ASCII files are read by a
//! lenient parser (it skips comment lines before `solid`, which some exporters write).

use super::{Loader, DEFAULT_COLOR, Source};
use crate::model::Layer;
use glam::Vec3;

pub struct StlLoader;

impl Loader for StlLoader {
    fn name(&self) -> &'static str {
        "STL"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["stl"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let data = source.read()?;
        let name = source.stem();
        let triangles = if is_binary(&data) { read_binary(&data)? } else { read_ascii(&data)? };
        let mut positions = Vec::with_capacity(triangles.len() * 9);
        let mut normals = Vec::with_capacity(triangles.len() * 9);
        for (stored, v) in triangles {
            let mut n = (v[1] - v[0]).cross(v[2] - v[0]).normalize_or_zero();
            if n == Vec3::ZERO {
                n = stored.normalize_or(Vec3::Z);
            }
            for p in v {
                positions.extend_from_slice(&p.to_array());
                normals.extend_from_slice(&n.to_array());
            }
        }
        let count = (positions.len() / 3) as u32;
        Ok(vec![Layer { name, color: DEFAULT_COLOR, positions, normals, indices: (0..count).collect(), ..Layer::default() }])
    }
}

type Triangle = (Vec3, [Vec3; 3]);

/// Binary STL: 80-byte header, u32 count, 50 bytes per triangle.
fn is_binary(data: &[u8]) -> bool {
    data.len() >= 84 && {
        let n = u32::from_le_bytes([data[80], data[81], data[82], data[83]]) as usize;
        84 + n * 50 == data.len()
    }
}

fn read_binary(data: &[u8]) -> Result<Vec<Triangle>, String> {
    let mut cursor = std::io::Cursor::new(data);
    let reader = stl_io::create_stl_reader(&mut cursor).map_err(|e| format!("STL: {e}"))?;
    let mut out = Vec::new();
    for t in reader {
        let t = t.map_err(|e| format!("STL: {e}"))?;
        out.push((Vec3::from_array(t.normal.0), t.vertices.map(|v| Vec3::from_array(v.0))));
    }
    Ok(out)
}

fn read_ascii(data: &[u8]) -> Result<Vec<Triangle>, String> {
    let text = String::from_utf8_lossy(data);
    let mut out = Vec::new();
    let mut normal = Vec3::ZERO;
    let mut verts: Vec<Vec3> = Vec::with_capacity(3);
    let parse3 = |it: &mut std::str::SplitWhitespace| -> Option<Vec3> {
        let mut v = [0.0f32; 3];
        for c in &mut v {
            *c = it.next()?.parse().ok()?;
        }
        Some(Vec3::from_array(v))
    };
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("facet") => {
                it.next(); // "normal"
                normal = parse3(&mut it).unwrap_or(Vec3::ZERO);
                verts.clear();
            }
            Some("vertex") => {
                verts.push(parse3(&mut it).ok_or("STL: bad vertex line")?);
                if verts.len() == 3 {
                    out.push((normal, [verts[0], verts[1], verts[2]]));
                    verts.clear();
                }
            }
            _ => {}
        }
    }
    if out.is_empty() {
        return Err("STL: no facets found".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_single_triangle() {
        let mut d = vec![0u8; 80];
        d.extend_from_slice(&1u32.to_le_bytes());
        for v in [0.0f32, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&[0, 0]);
        assert!(is_binary(&d));
        let t = read_binary(&d).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].1[1], Vec3::X);
    }

    #[test]
    fn ascii_with_leading_comments() {
        let s = b";; comment\nsolid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid x\n";
        assert!(!is_binary(s));
        assert_eq!(read_ascii(s).unwrap().len(), 1);
    }
}
