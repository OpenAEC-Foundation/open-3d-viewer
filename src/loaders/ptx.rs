//! Leica PTX point clouds (`.ptx`), as Cyclone, Register 360 and many scanners export them: a
//! scan per block (columns, rows, the scanner position and axes, a 4 x 4 registration matrix,
//! then a point per line: X Y Z intensity [R G B]). Every scan is a layer, its points moved by
//! the matrix into the registered frame; points without a return (0 0 0) are left out. Points take
//! their colour, else their intensity as grey. Thinned out evenly to
//! [`MAX_POINTS`](super::pointcloud::MAX_POINTS); far coordinates move to a local origin.

use super::pointcloud::{grey, Cloud, MAX_POINTS};
use super::xyz::lines;
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};

pub struct PtxLoader;

impl Loader for PtxLoader {
    fn name(&self) -> &'static str {
        "PTX"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ptx"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        let (layers, info) = read(&data, &source.stem(), MAX_POINTS).map_err(|e| format!("{}: {e}", source.name()))?;
        Ok(Loaded { layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } })
    }
}

/// Scan colours for the layer list.
const SCAN_COLORS: [[f32; 3]; 6] =
    [[0.80, 0.80, 0.82], [0.55, 0.72, 0.90], [0.90, 0.70, 0.45], [0.60, 0.82, 0.55], [0.85, 0.58, 0.70], [0.70, 0.65, 0.88]];

fn numbers(line: &[u8]) -> Vec<f64> {
    std::str::from_utf8(line).unwrap_or("").split_ascii_whitespace().map_while(|w| w.parse().ok()).collect()
}

/// Reads every scan (keeping at most `max` points over all of them).
fn read(data: &[u8], stem: &str, max: usize) -> Result<(Vec<Layer>, Vec<(String, String)>), String> {
    let estimate = data.iter().filter(|&&b| b == b'\n').count() as u64 + 1;
    let mut cloud = Cloud::new(estimate, max);
    let mut lines = lines(data).filter(|l| !l.iter().all(u8::is_ascii_whitespace));
    let mut scans = 0u32;
    let mut total = 0u64;
    let mut empty = 0u64;
    let mut colored = false;
    while let Some(first) = lines.next() {
        let header: Vec<Vec<f64>> = std::iter::once(first).chain(lines.by_ref().take(9)).map(numbers).collect();
        let size = |k: usize| header.get(k).and_then(|v| (v.len() == 1).then(|| v[0])).filter(|v| *v >= 0.0 && v.fract() == 0.0);
        let (Some(columns), Some(rows)) = (size(0), size(1)) else {
            return Err(match scans {
                0 => "not a PTX file (it starts with the columns and rows of a scan)".into(),
                n => format!("scan {} has no columns and rows", n + 1),
            });
        };
        if header.len() < 10 || header[2..6].iter().any(|v| v.len() < 3) || header[6..10].iter().any(|v| v.len() < 4) {
            return Err(format!("scan {}: incomplete header", scans + 1));
        }
        // row vector times matrix: the translation is the fourth row
        let m: Vec<&[f64]> = header[6..10].iter().map(Vec::as_slice).collect();
        let transform = |p: [f64; 3]| -> [f64; 3] { [0, 1, 2].map(|j| p[0] * m[0][j] + p[1] * m[1][j] + p[2] * m[2][j] + m[3][j]) };
        let n = scans;
        scans += 1;
        let base = SCAN_COLORS[n as usize % SCAN_COLORS.len()];
        let layer = cloud.layer(n, || Layer { name: format!("Scan {}", n + 1), color: base, ..Layer::default() });
        for _ in 0..(columns * rows) as u64 {
            let Some(line) = lines.next() else { break };
            if !cloud.keep() {
                total += 1;
                continue;
            }
            let v = numbers(line);
            if v.len() < 3 {
                continue;
            }
            if v[0] == 0.0 && v[1] == 0.0 && v[2] == 0.0 {
                empty += 1;
                continue;
            }
            total += 1;
            let color = if v.len() >= 7 {
                colored = true;
                [v[4], v[5], v[6]].map(|c| c.round().clamp(0.0, 255.0) as u8)
            } else {
                grey(v.get(3).copied().unwrap_or(0.5), (0.0, 1.0))
            };
            cloud.push(layer, transform([v[0], v[1], v[2]]), color);
        }
    }
    if scans == 0 {
        return Err("empty file".into());
    }
    let mut info = cloud.info("PTX", total);
    info.insert(0, ("Scans".into(), scans.to_string()));
    if empty > 0 {
        info.push(("Points without a return (left out)".into(), empty.to_string()));
    }
    info.push(("Point colours".into(), if colored { "RGB" } else { "Intensity" }.into()));
    let mut layers = cloud.into_layers(true);
    if let [layer] = layers.as_mut_slice() {
        layer.name = stem.to_string();
    }
    Ok((layers, info))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::{load_with, Progress, Source};
    use std::sync::Arc;

    /// Two scans of 2 x 2 points: the first with colours and the identity, the second with
    /// intensities only, turned a quarter about Z and moved 10 m along X, with one empty point.
    const PTX: &str = "2\n2\n0 0 0\n1 0 0\n0 1 0\n0 0 1\n1 0 0 0\n0 1 0 0\n0 0 1 0\n0 0 0 1\n\
        1 0 0 0.5 255 0 0\n2 0 0 0.5 255 0 0\n1 1 0 0.5 0 255 0\n2 1 0 0.5 0 255 0\n\
        2\n2\n10 0 0\n0 1 0\n-1 0 0\n0 0 1\n0 1 0 0\n-1 0 0 0\n0 0 1 0\n10 0 0 1\n\
        1 0 2 1.0\n2 0 2 0.0\n0 0 0 0.5\n3 0 2 0.5\n";

    #[test]
    fn places_scans_by_their_matrix() {
        let source = Source::Bytes { name: "survey.PTX".into(), data: Arc::new(PTX.as_bytes().to_vec()) };
        let model = load_with(&source, &Progress::default()).expect("PTX");
        assert_eq!(model.format, "PTX");
        assert_eq!(model.point_count(), 7);
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Scan 1", "Scan 2"]);
        let first = &model.layers[0];
        assert_eq!(&first.positions[..3], &[1.0, 0.0, 0.0]);
        assert_eq!(first.colors.as_ref().unwrap()[0], [255, 0, 0, 255]);
        // (1, 0, 2) turned a quarter about Z is (0, 1, 2), moved to (10, 1, 2)
        let second = &model.layers[1];
        assert_eq!(&second.positions[..3], &[10.0, 1.0, 2.0]);
        assert_eq!(&second.positions[6..9], &[10.0, 3.0, 2.0]);
        assert_eq!(second.colors.as_ref().unwrap()[0], [255, 255, 255, 255]);
        assert_eq!(second.colors.as_ref().unwrap()[1], [0, 0, 0, 255]);
        let info = &model.project.info;
        assert!(info.iter().any(|(k, v)| k == "Scans" && v == "2"));
        assert!(info.iter().any(|(k, v)| k.contains("without a return") && v == "1"));
    }

    #[test]
    fn thins_out_and_rejects_other_text() {
        let (layers, info) = read(PTX.as_bytes(), "s", 4).unwrap();
        // 28 lines, at most 4 points: every 8th line is kept
        assert!(layers.iter().map(Layer::vertex_count).sum::<usize>() <= 4);
        assert!(info.iter().any(|(k, _)| k == "Points shown"));
        assert!(read(b"hello world\n", "s", 10).is_err());
    }
}
