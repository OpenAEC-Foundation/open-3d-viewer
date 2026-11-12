//! The map of where a model lies: the OpenStreetMap tiles around a latitude and longitude, put
//! together to one picture with a dot on the place.
//!
//! This asks the tile server for the tiles around the place, so the place is known there; nothing
//! else of the model or the user goes along (the request names the app and its version only). It
//! is asked for when the user asks for it, once per place while the model is open. Without a network there is no map and
//! the pane says so. The browser build shows no map.

/// The zoom of the tiles: a street and its surroundings.
const ZOOM: u32 = 16;
/// The side of a tile (pixels).
const TILE: u32 = 256;
/// The picture cut from the 3 by 3 tiles around the place (pixels).
pub const SIZE: (u32, u32) = (512, 320);

/// Where a latitude and longitude lie in the tile grid of a zoom (tile units, fractions within a tile).
fn tile_point(lat: f64, lon: f64, zoom: u32) -> (f64, f64) {
    let n = f64::from(1u32 << zoom);
    let lat = lat.clamp(-85.0511, 85.0511).to_radians();
    ((lon + 180.0) / 360.0 * n, (1.0 - lat.tan().asinh() / std::f64::consts::PI) / 2.0 * n)
}

/// The picture of the place as RGBA rows, [`SIZE`] large, or why there is none.
#[cfg(not(target_arch = "wasm32"))]
pub fn fetch(lat: f64, lon: f64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let (x, y) = tile_point(lat, lon, ZOOM);
    let (tx, ty) = (x.floor() as i64, y.floor() as i64);
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(concat!("Open3DViewerStudio/", env!("CARGO_PKG_VERSION")))
        .build();
    // the 3 by 3 tiles around the place, in one picture
    let side = (TILE * 3) as usize;
    let mut sheet = vec![0u8; side * side * 4];
    let last = (1i64 << ZOOM) - 1;
    for (row, dy) in (-1..=1).enumerate() {
        for (column, dx) in (-1..=1).enumerate() {
            let (cx, cy) = ((tx + dx).rem_euclid(last + 1), ty + dy);
            if !(0..=last).contains(&cy) {
                continue;
            }
            let url = format!("https://tile.openstreetmap.org/{ZOOM}/{cx}/{cy}.png");
            let response = agent.get(&url).call().map_err(|e| format!("{e}"))?;
            let mut bytes = Vec::new();
            response.into_reader().take(4 << 20).read_to_end(&mut bytes).map_err(|e| format!("{e}"))?;
            let tile = image::load_from_memory(&bytes).map_err(|e| format!("{e}"))?.to_rgba8();
            if tile.width() != TILE || tile.height() != TILE {
                return Err("the map tile has another size than expected".into());
            }
            for line in 0..TILE as usize {
                let from = line * TILE as usize * 4;
                let to = ((row * TILE as usize + line) * side + column * TILE as usize) * 4;
                sheet[to..to + TILE as usize * 4].copy_from_slice(&tile.as_raw()[from..from + TILE as usize * 4]);
            }
        }
    }
    // the place in the picture of the nine tiles, and the cut around it
    let (px, py) = (((x - tx as f64 + 1.0) * f64::from(TILE)) as i64, ((y - ty as f64 + 1.0) * f64::from(TILE)) as i64);
    let (w, h) = (i64::from(SIZE.0), i64::from(SIZE.1));
    let (left, top) = ((px - w / 2).clamp(0, side as i64 - w), (py - h / 2).clamp(0, side as i64 - h));
    let mut out = vec![0u8; (w * h * 4) as usize];
    for line in 0..h {
        let from = (((top + line) * side as i64 + left) * 4) as usize;
        let to = (line * w * 4) as usize;
        out[to..to + (w * 4) as usize].copy_from_slice(&sheet[from..from + (w * 4) as usize]);
    }
    // the dot: red with a white rim
    let (cx, cy) = (px - left, py - top);
    for dy in -8i64..=8 {
        for dx in -8i64..=8 {
            let (qx, qy, r2) = (cx + dx, cy + dy, dx * dx + dy * dy);
            if r2 > 64 || !(0..w).contains(&qx) || !(0..h).contains(&qy) {
                continue;
            }
            let at = ((qy * w + qx) * 4) as usize;
            out[at..at + 4].copy_from_slice(if r2 > 30 { &[255, 255, 255, 255] } else { &[214, 40, 40, 255] });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_lies_in_the_tile_grid() {
        // the tile grid's middle is latitude 0, longitude 0
        let (x, y) = tile_point(0.0, 0.0, 1);
        assert!((x - 1.0).abs() < 1e-9 && (y - 1.0).abs() < 1e-9);
        // Amersfoort, zoom 16: a known tile column and row
        let (x, y) = tile_point(52.155_174, 5.387_206, 16);
        assert_eq!((x.floor() as i64, y.floor() as i64), (33748, 21601));
    }
}
