//! E57 point clouds (ASTM E2807) through the pure Rust `e57` crate: every scan is a point layer,
//! placed by its pose; spherical coordinates are turned into Cartesian ones. Points take their
//! colour, else their intensity as grey, else the layer colour. Thinned out evenly to
//! [`MAX_POINTS`](super::pointcloud::MAX_POINTS) over all scans; far coordinates move to a local
//! origin. Images in the file are not read.

use super::pointcloud::{Cloud, MAX_POINTS};
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};
use std::io::{Read, Seek};

pub struct E57Loader;

impl Loader for E57Loader {
    fn name(&self) -> &'static str {
        "E57"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["e57"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let error = |e: ::e57::Error| format!("{}: {e}", source.name());
        let (layers, info) = match source {
            Source::File(path) => read(::e57::E57Reader::from_file(path).map_err(error)?, MAX_POINTS),
            Source::Bytes { data, .. } => read(::e57::E57Reader::new(std::io::Cursor::new(data.as_slice())).map_err(error)?, MAX_POINTS),
        }
        .map_err(error)?;
        progress.set(Phase::Convert);
        Ok(Loaded { layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } })
    }
}

/// Scan colours for the layer list (and points without colour or intensity).
const SCAN_COLORS: [[f32; 3]; 6] =
    [[0.80, 0.80, 0.82], [0.55, 0.72, 0.90], [0.90, 0.70, 0.45], [0.60, 0.82, 0.55], [0.85, 0.58, 0.70], [0.70, 0.65, 0.88]];

/// Reads the points of every scan (keeping at most `max` over all of them).
fn read<T: Read + Seek>(mut reader: ::e57::E57Reader<T>, max: usize) -> ::e57::Result<(Vec<Layer>, Vec<(String, String)>)> {
    let scans = reader.pointclouds();
    let total: u64 = scans.iter().map(|s| s.records).sum();
    let mut cloud = Cloud::new(total, max);
    for (n, scan) in scans.iter().enumerate() {
        let name = scan.name.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| format!("Scan {}", n + 1));
        let base = SCAN_COLORS[n % SCAN_COLORS.len()];
        let layer = cloud.layer(n as u32, || Layer { name, color: base, ..Layer::default() });
        let base = base.map(|c| (c * 255.0).round() as u8);
        // pose applied, spherical to Cartesian, intensity as grey where there is no colour
        let points = reader.pointcloud_simple(scan)?;
        for point in points {
            let point = point?;
            if !cloud.keep() {
                continue;
            }
            let ::e57::CartesianCoordinate::Valid { x, y, z } = point.cartesian else { continue };
            let color = match (&point.color, point.intensity) {
                (Some(c), _) => [c.red, c.green, c.blue].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
                (None, Some(i)) => [(i.clamp(0.0, 1.0) * 255.0).round() as u8; 3],
                (None, None) => base,
            };
            cloud.push(layer, [x, y, z], color);
        }
    }
    let mut info = cloud.info("E57", total);
    info.insert(0, ("Scans".into(), scans.len().to_string()));
    if let Some(sensor) = scans.iter().find_map(|s| {
        let parts: Vec<&str> = [&s.sensor_vendor, &s.sensor_model].into_iter().flatten().map(|v| v.trim()).filter(|v| !v.is_empty()).collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    }) {
        info.push(("Scanner".into(), sensor));
    }
    Ok((cloud.into_layers(false), info))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::{load_with, Progress, Source};
    use ::e57::{Record, RecordDataType, RecordName, RecordValue, Transform, Translation, Quaternion};
    use std::sync::Arc;

    /// An E57 file with two scans: a Cartesian one with colours, moved by its pose, and a
    /// spherical one with intensities only.
    fn sample() -> Vec<u8> {
        let mut file = std::io::Cursor::new(Vec::new());
        let mut writer = ::e57::E57Writer::new(&mut file, "file-guid").unwrap();
        let color = |name| Record { name, data_type: RecordDataType::Integer { min: 0, max: 255 } };
        let prototype = vec![
            Record::CARTESIAN_X_F64,
            Record::CARTESIAN_Y_F64,
            Record::CARTESIAN_Z_F64,
            color(RecordName::ColorRed),
            color(RecordName::ColorGreen),
            color(RecordName::ColorBlue),
        ];
        let mut scan = writer.add_pointcloud("scan-1", prototype).unwrap();
        scan.set_name(Some("Hall".into()));
        // turned a quarter about Z, then moved 10 m along X
        let half = std::f64::consts::FRAC_1_SQRT_2;
        scan.set_transform(Some(Transform {
            rotation: Quaternion { w: half, x: 0.0, y: 0.0, z: half },
            translation: Translation { x: 10.0, y: 0.0, z: 0.0 },
        }));
        for i in 0..10 {
            scan.add_point(vec![
                RecordValue::Double(i as f64),
                RecordValue::Double(0.0),
                RecordValue::Double(1.0),
                RecordValue::Integer(255),
                RecordValue::Integer(0),
                RecordValue::Integer(0),
            ])
            .unwrap();
        }
        scan.finalize().unwrap();
        let prototype = vec![
            Record::SPHERICAL_RANGE_F64,
            Record::SPHERICAL_AZIMUTH_F64,
            Record::SPHERICAL_ELEVATION_F64,
            Record { name: RecordName::Intensity, data_type: RecordDataType::Integer { min: 0, max: 100 } },
        ];
        let mut scan = writer.add_pointcloud("scan-2", prototype).unwrap();
        for i in 0..5 {
            // straight up, 2 m away
            scan.add_point(vec![
                RecordValue::Double(2.0),
                RecordValue::Double(0.0),
                RecordValue::Double(std::f64::consts::FRAC_PI_2),
                RecordValue::Integer(i * 25),
            ])
            .unwrap();
        }
        scan.finalize().unwrap();
        writer.finalize().unwrap();
        drop(writer);
        file.into_inner()
    }

    #[test]
    fn reads_scans_with_their_poses() {
        let source = Source::Bytes { name: "survey.E57".into(), data: Arc::new(sample()) };
        let model = load_with(&source, &Progress::default()).expect("E57");
        assert_eq!(model.format, "E57");
        assert_eq!(model.point_count(), 15);
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Hall", "Scan 2"]);
        let hall = &model.layers[0];
        // x = 9 turned onto +Y, then moved to x = 10
        let p = &hall.positions[27..30];
        assert!((p[0] - 10.0).abs() < 1e-5 && (p[1] - 9.0).abs() < 1e-5 && (p[2] - 1.0).abs() < 1e-5, "{p:?}");
        assert_eq!(hall.colors.as_ref().unwrap()[3], [255, 0, 0, 255]);
        let spherical = &model.layers[1];
        assert!(spherical.positions[2] > 1.99 && spherical.positions[0].abs() < 1e-6);
        // intensity as grey, brighter along the scan
        let grey = spherical.colors.as_ref().unwrap();
        assert_eq!(grey[0], [0, 0, 0, 255]);
        assert_eq!(grey[4], [255, 255, 255, 255]);
        assert!(model.project.info.iter().any(|(k, v)| k == "Scans" && v == "2"));
    }

    /// `O3V_E57_SAMPLE=file.e57 cargo test write_sample_file -- --ignored`: writes a synthetic
    /// survey of a room (floor, walls, a column) from two scanner stations, spherical with
    /// intensity, the second station turned and moved by its pose, for screenshots and timings.
    #[test]
    #[ignore]
    fn write_sample_file() {
        let Some(path) = std::env::var_os("O3V_E57_SAMPLE") else { return };
        let mut writer = ::e57::E57Writer::from_file(path, "sample-guid").unwrap();
        let (w, d, h) = (12.0f64, 8.0f64, 3.0f64);
        for (k, (station, turn)) in [([3.0, 3.0], 0.0f64), ([9.0, 5.0], 2.0)].into_iter().enumerate() {
            let prototype = vec![
                Record::SPHERICAL_RANGE_F64,
                Record::SPHERICAL_AZIMUTH_F64,
                Record::SPHERICAL_ELEVATION_F64,
                Record { name: RecordName::Intensity, data_type: RecordDataType::Integer { min: 0, max: 1000 } },
            ];
            let mut scan = writer.add_pointcloud(&format!("station-{k}"), prototype).unwrap();
            scan.set_name(Some(format!("Station {}", k + 1)));
            let (s, c) = ((turn / 2.0).sin(), (turn / 2.0).cos());
            scan.set_transform(Some(Transform {
                rotation: Quaternion { w: c, x: 0.0, y: 0.0, z: s },
                translation: Translation { x: station[0], y: station[1], z: 1.5 },
            }));
            // rays in the scanner frame, cast into the room (world) to the nearest surface
            for i in 0..600 {
                for j in 0..300 {
                    let az = -std::f64::consts::PI + i as f64 * std::f64::consts::TAU / 600.0;
                    let el = -1.2 + j as f64 * 2.6 / 300.0;
                    let dir = [el.cos() * (az + turn).cos(), el.cos() * (az + turn).sin(), el.sin()];
                    let origin = [station[0], station[1], 1.5];
                    let mut range = f64::INFINITY;
                    for (axis, lo, hi) in [(0, 0.0, w), (1, 0.0, d), (2, 0.0, h)] {
                        if dir[axis].abs() > 1e-9 {
                            let wall = if dir[axis] > 0.0 { hi } else { lo };
                            range = range.min((wall - origin[axis]) / dir[axis]);
                        }
                    }
                    // a round column of 0.4 m at (6, 4)
                    let (ox, oy) = (origin[0] - 6.0, origin[1] - 4.0);
                    let (a, b) = (dir[0] * dir[0] + dir[1] * dir[1], 2.0 * (ox * dir[0] + oy * dir[1]));
                    let disc = b * b - 4.0 * a * (ox * ox + oy * oy - 0.16);
                    if disc > 0.0 {
                        let t = (-b - disc.sqrt()) / (2.0 * a);
                        if t > 0.0 {
                            range = range.min(t);
                        }
                    }
                    let intensity = (300.0 + 600.0 / (1.0 + range) + 100.0 * (i % 7) as f64 / 7.0) as i64;
                    scan.add_point(vec![
                        RecordValue::Double(range),
                        RecordValue::Double(az),
                        RecordValue::Double(el),
                        RecordValue::Integer(intensity.min(1000)),
                    ])
                    .unwrap();
                }
            }
            scan.finalize().unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn thins_out_over_all_scans() {
        let reader = ::e57::E57Reader::new(std::io::Cursor::new(sample())).unwrap();
        let (layers, _) = read(reader, 5).unwrap();
        // every third of 15 points: 4 of the first scan, 1 of the second
        assert_eq!(layers.iter().map(Layer::vertex_count).collect::<Vec<_>>(), [4, 1]);
    }
}
