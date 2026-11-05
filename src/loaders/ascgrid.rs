//! ESRI ASCII grids (`.asc` that start with `ncols`), as elevation models are handed out
//! (AHN, Copernicus, national mapping agencies): a terrain mesh of two triangles per cell between
//! cell centres, cells with the `NODATA_value` left out, smooth normals. A grid of more than
//! [`MAX_CELLS`] cells is sampled every n-th row and column. Coordinates are moved to a local
//! origin at the low corner of the grid; the origin is in the project information. Read by the
//! XYZ loader, which owns the `.asc` extension.

use crate::model::Layer;
use glam::Vec3;

/// At most this many cells become triangles (two each).
pub const MAX_CELLS: usize = if cfg!(target_arch = "wasm32") { 2_000_000 } else { 6_000_000 };

/// Whether the file is an ESRI ASCII grid.
pub fn is_grid(data: &[u8]) -> bool {
    let start = &data[..data.len().min(64)];
    String::from_utf8_lossy(start).trim_start_matches('\u{feff}').trim_start().get(..5).is_some_and(|w| w.eq_ignore_ascii_case("ncols"))
}

/// Reads the grid: the terrain layer and the project information.
pub fn read(data: &[u8], stem: &str, max_cells: usize) -> Result<(Vec<Layer>, Vec<(String, String)>), String> {
    let text = String::from_utf8_lossy(data);
    let mut words = text.split_ascii_whitespace();
    let (mut ncols, mut nrows, mut x0, mut y0, mut cell, mut nodata) = (0usize, 0usize, f64::NAN, f64::NAN, f64::NAN, None);
    let (mut dx, mut dy) = (None, None);
    let mut center = false;
    let mut first_value = None;
    // the header: keyword value pairs, until the first number
    while let Some(w) = words.next() {
        if w.parse::<f64>().is_ok() {
            first_value = Some(w);
            break;
        }
        let value = words.next().ok_or("ESRI grid: incomplete header")?;
        let number = || value.parse::<f64>().map_err(|_| format!("ESRI grid: bad {w} {value}"));
        match w.to_ascii_lowercase().as_str() {
            "ncols" => ncols = number()? as usize,
            "nrows" => nrows = number()? as usize,
            "xllcorner" => x0 = number()?,
            "yllcorner" => y0 = number()?,
            "xllcenter" => (x0, center) = (number()?, true),
            "yllcenter" => (y0, center) = (number()?, true),
            "cellsize" => cell = number()?,
            "dx" => dx = Some(number()?),
            "dy" => dy = Some(number()?),
            "nodata_value" => nodata = Some(number()?),
            _ => {}
        }
    }
    let (dx, dy) = (dx.unwrap_or(cell), dy.unwrap_or(cell));
    if ncols < 2 || nrows < 2 || !(x0.is_finite() && y0.is_finite() && dx > 0.0 && dy > 0.0) {
        return Err("ESRI grid: header without ncols, nrows, corner and cell size".into());
    }
    let values: Vec<f32> = first_value.into_iter().chain(words).take(ncols * nrows).map(|v| v.parse::<f64>().unwrap_or(f64::NAN) as f32).collect();
    if values.len() < ncols * nrows {
        return Err(format!("ESRI grid: {} of {} values", values.len(), ncols * nrows));
    }
    let nodata = nodata.map(|v| v as f32);
    let valid = |v: f32| v.is_finite() && Some(v) != nodata;
    // every n-th row and column for large grids
    let step = (((ncols * nrows) as f64 / max_cells.max(1) as f64).sqrt().ceil() as usize).max(1);
    let (cols, rows) = ((ncols - 1) / step + 1, (nrows - 1) / step + 1);
    // the first cell centre (rows run from the top down)
    let (cx, cy) = if center { (x0, y0) } else { (x0 + dx / 2.0, y0 + dy / 2.0) };
    let origin = [x0.floor(), y0.floor()];
    let at = |c: usize, r: usize| -> Option<Vec3> {
        let v = values[r * ncols + c];
        valid(v).then(|| Vec3::new((cx + c as f64 * dx - origin[0]) as f32, (cy + (nrows - 1 - r) as f64 * dy - origin[1]) as f32, v))
    };
    let mut layer = Layer { name: stem.to_string(), color: [0.70, 0.72, 0.62], ..Layer::default() };
    // a vertex per sampled post
    let mut index = vec![u32::MAX; cols * rows];
    for r in 0..rows {
        for c in 0..cols {
            if let Some(p) = at(c * step, r * step) {
                index[r * cols + c] = layer.vertex_count() as u32;
                layer.positions.extend_from_slice(&p.to_array());
            }
        }
    }
    let point = |i: u32| Vec3::from_slice(&layer.positions[3 * i as usize..3 * i as usize + 3]);
    let mut indices = Vec::new();
    for r in 0..rows - 1 {
        for c in 0..cols - 1 {
            // counter-clockwise seen from above: rows run southwards
            let (a, b, d, e) = (index[r * cols + c], index[r * cols + c + 1], index[(r + 1) * cols + c], index[(r + 1) * cols + c + 1]);
            for t in [[a, d, e], [a, e, b]] {
                if t.iter().all(|&i| i != u32::MAX) {
                    indices.extend_from_slice(&t);
                }
            }
        }
    }
    if indices.is_empty() {
        return Err("ESRI grid: no cells with data".into());
    }
    let mut normals = vec![Vec3::ZERO; layer.vertex_count()];
    for t in indices.chunks_exact(3) {
        let n = (point(t[1]) - point(t[0])).cross(point(t[2]) - point(t[0]));
        for &i in t {
            normals[i as usize] += n;
        }
    }
    layer.normals = normals.into_iter().flat_map(|n| n.normalize_or(Vec3::Z).to_array()).collect();
    layer.indices = indices;
    let mut info = vec![
        ("ESRI grid".into(), format!("{ncols} x {nrows} cells of {dx} x {dy}")),
        ("ESRI grid world origin (source coordinates)".into(), format!("{}, {}, 0", origin[0], origin[1])),
    ];
    if step > 1 {
        info.push(("Cells shown".into(), format!("every {step}th row and column")));
    }
    Ok((vec![layer], info))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: &str = "ncols 3\nnrows 2\nxllcorner 155000\nyllcorner 463000\ncellsize 1\nNODATA_value -9999\n1 2 3\n4 5 -9999\n";

    #[test]
    fn reads_a_grid_as_a_terrain() {
        assert!(is_grid(GRID.as_bytes()));
        let (layers, info) = read(GRID.as_bytes(), "dtm", MAX_CELLS).unwrap();
        let layer = &layers[0];
        // 5 posts with data; the second cell misses the corner both its triangles share
        assert_eq!(layer.vertex_count(), 5);
        assert_eq!(layer.triangle_count(), 2);
        // the top row is the northern one: (0.5, 1.5, 1)
        assert_eq!(&layer.positions[..3], &[0.5, 1.5, 1.0]);
        assert!(layer.normals.chunks_exact(3).all(|n| n[2] > 0.0), "faces up");
        assert!(info.iter().any(|(_, v)| v == "155000, 463000, 0"));
        // 5 x 5 posts, at most 4 cells: every 3rd row and column, 2 x 2 posts
        let big = format!("ncols 5
nrows 5
xllcenter 0
yllcenter 0
cellsize 2
{}", "0 ".repeat(25));
        let (layers, info) = read(big.as_bytes(), "dtm", 4).unwrap();
        assert_eq!((layers[0].vertex_count(), layers[0].triangle_count()), (4, 2));
        assert_eq!(layers[0].positions.chunks_exact(3).map(|p| p[0]).fold(0.0, f32::max), 6.0);
        assert!(info.iter().any(|(k, _)| k == "Cells shown"));
    }
}
