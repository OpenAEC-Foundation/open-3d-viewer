//! ASTM E57 point cloud import using the `e57` crate.
//!
//! Every scan becomes one group (layer) of points with its pose applied. Points keep their
//! RGB colour, or intensity as grey when there is no colour. Large files are thinned evenly
//! to at most `MAX_POINTS` points in total.

use super::encode::{Scene, KIND_POINTS};
use e57::{CartesianCoordinate, E57Reader};
use std::collections::HashSet;
use std::io::Cursor;

pub const MAX_POINTS: u64 = 5_000_000;

/// Colour of scans without colour or intensity.
const PLAIN_COLOR: [f32; 3] = [0.53, 0.8, 1.0];

pub fn convert(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let scene = scene(bytes)?;
    if scene.is_empty() {
        return Err("The E57 file contains no valid points".to_string());
    }
    Ok(scene.encode())
}

pub fn scene(bytes: &[u8]) -> Result<Scene, String> {
    if bytes.len() < 8 || &bytes[0..8] != b"ASTM-E57" {
        return Err("Not an E57 file (missing the ASTM-E57 signature)".to_string());
    }
    let mut reader = E57Reader::new(Cursor::new(bytes)).map_err(|e| format!("Cannot read E57 file: {e}"))?;
    let clouds = reader.pointclouds();
    if clouds.is_empty() {
        return Err("The E57 file contains no point clouds".to_string());
    }
    let total: u64 = clouds.iter().map(|pc| pc.records).sum();
    let step = total.div_ceil(MAX_POINTS).max(1);

    let mut scene = Scene::new();
    let mut names = HashSet::new();
    let mut kept_total = 0u64;
    let mut failed = Vec::new();
    for (i, pc) in clouds.iter().enumerate() {
        let mut name = pc.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("Scan {}", i + 1));
        if !names.insert(name.clone()) {
            name = format!("{name} ({})", i + 1);
            names.insert(name.clone());
        }
        let has_color = pc.has_color() || pc.has_intensity();
        let mut iter = match reader.pointcloud_simple(pc) {
            Ok(it) => it,
            Err(e) => {
                failed.push(format!("{name}: {e}"));
                continue;
            }
        };
        iter.apply_pose(true);
        iter.spherical_to_cartesian(true);
        iter.intensity_to_color(true);

        let item = scene.item(&name, KIND_POINTS, PLAIN_COLOR);
        let expected = (pc.records / step + 1) as usize;
        item.positions.reserve(expected * 3);
        if has_color {
            item.colors.reserve(expected * 3);
        }
        let mut index = 0u64;
        for p in iter {
            let p = match p {
                Ok(p) => p,
                Err(e) => {
                    failed.push(format!("{name}: {e}"));
                    break;
                }
            };
            let keep = index % step == 0;
            index += 1;
            if !keep {
                continue;
            }
            let CartesianCoordinate::Valid { x, y, z } = p.cartesian else {
                continue;
            };
            if !(x.is_finite() && y.is_finite() && z.is_finite()) {
                continue;
            }
            item.positions.extend_from_slice(&[x, y, z]);
            if has_color {
                let c = match (&p.color, p.intensity) {
                    (Some(c), _) => [c.red, c.green, c.blue],
                    (None, Some(i)) => [i, i, i],
                    (None, None) => PLAIN_COLOR,
                };
                item.colors.extend(c.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
            }
        }
        kept_total += (item.positions.len() / 3) as u64;
    }
    scene.notes.push(format!(
        "E57: {} scan(s), {} points in file, {} shown{}",
        clouds.len(),
        total,
        kept_total,
        if step > 1 { format!(" (1 in {step} points)") } else { String::new() }
    ));
    for f in failed {
        scene.notes.push(format!("Read error in {f}"));
    }
    Ok(scene)
}
