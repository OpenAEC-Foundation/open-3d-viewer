//! LAS and LAZ point clouds (ASPRS LAS 1.0 to 1.4, LASzip compressed) through the `las` crate
//! with `laz`, both pure Rust. One point layer per classification class (Ground, Building, ...)
//! in the ASPRS class colours; points take their RGB colour when the file has one, else the
//! colour of their class shaded by their intensity. Read in chunks, thinned out evenly to
//! [`MAX_POINTS`](super::pointcloud::MAX_POINTS); survey coordinates move to a local origin.
//!
//! A cloud-optimised file (COPC, `.copc.laz`, as AHN6 is delivered) is read through its octree
//! with the `las` crate's COPC reader: whole levels from the coarsest down while they fit in the
//! point limit, and the next level thinned out to fill it. Each level of the octree is an even
//! sample of the whole cloud, so a large file shows quickly and evenly without decompressing
//! every point.

use super::pointcloud::{Cloud, MAX_POINTS};
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};
use std::sync::Arc;

pub struct LasLoader;

impl Loader for LasLoader {
    fn name(&self) -> &'static str {
        "LAS"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["las", "laz"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let err = |e: ::las::Error| format!("{}: {e}", source.name());
        let reader = match source {
            Source::File(path) => ::las::Reader::from_path(path),
            Source::Bytes { data, .. } => ::las::Reader::new(std::io::Cursor::new(Bytes(data.clone()))),
        }
        .map_err(err)?;
        let stem = source.stem();
        let stem = stem.strip_suffix(".copc").unwrap_or(&stem);
        let (layers, info) = if reader.header().copc_info_vlr().is_some() {
            drop(reader);
            match source {
                Source::File(path) => read_copc(::las::CopcReader::from_path(path).map_err(err)?, stem, MAX_POINTS),
                Source::Bytes { data, .. } => read_copc(::las::CopcReader::new(std::io::Cursor::new(Bytes(data.clone()))).map_err(err)?, stem, MAX_POINTS),
            }
        } else {
            read(reader, stem, MAX_POINTS)
        }
        .map_err(err)?;
        progress.set(Phase::Convert);
        Ok(Loaded { structural: Vec::new(), origin: None, layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } })
    }
}

/// The bytes of a file picked in the browser, read in place.
struct Bytes(Arc<Vec<u8>>);

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Points per chunk: a few megabytes of records at a time.
const CHUNK: u64 = 1 << 18;

/// Reads every point (keeping at most `max`) into a layer per class, with the project
/// information about the file.
fn read(mut reader: ::las::Reader, stem: &str, max: usize) -> Result<(Vec<Layer>, Vec<(String, String)>), ::las::Error> {
    let header = reader.header().clone();
    let total = header.number_of_points();
    let mut cloud = Cloud::new(total, max);
    let b = header.bounds();
    cloud.anchor([b.min.x, b.min.y, b.min.z]);
    let mut data = ::las::PointDataBuilder::new().for_header(&header).build();
    let mut colours = Colours::default();
    loop {
        let n = reader.fill_points(CHUNK, &mut data)?;
        if n == 0 {
            break;
        }
        colours.add(&mut cloud, &data, Cloud::keep);
    }
    let info = cloud.info("LAS", total);
    Ok(finish(cloud, info, &header, stem))
}

/// How the colours of a file are read: RGB in 16 bits as the standard says, or 8 bits as some
/// writers store it, decided by the first colours that are not black; intensity scaled by the
/// brightest of the first points.
#[derive(Default)]
struct Colours {
    rgb_shift: Option<u32>,
    intensity_scale: Option<f32>,
}

impl Colours {
    /// Adds the points of `data` that `keep` keeps to `cloud`, a layer per class.
    fn add(&mut self, cloud: &mut Cloud, data: &::las::PointData, mut keep: impl FnMut(&mut Cloud) -> bool) {
        let rgb_max = data.rgb().and_then(|c| c.map(|(r, g, b)| r.max(g).max(b)).max()).unwrap_or(0);
        if self.rgb_shift.is_none() && rgb_max > 0 {
            self.rgb_shift = Some(if rgb_max > 255 { 8 } else { 0 });
        }
        let rgb_shift = self.rgb_shift;
        let intensity_scale = *self.intensity_scale.get_or_insert_with(|| 1.0 / data.intensity().max().unwrap_or(0).max(1) as f32);
        let mut rgb = data.rgb();
        let mut intensity = data.intensity();
        for ((x, y), (z, class)) in data.x().zip(data.y()).zip(data.z().zip(data.classification())) {
            let rgb = rgb.as_mut().and_then(Iterator::next);
            let i = intensity.next().unwrap_or(0);
            if !keep(cloud) {
                continue;
            }
            let layer = cloud.layer(class as u32, || Layer { name: class_name(class), color: class_color(class), ..Layer::default() });
            let color = match (rgb, rgb_shift) {
                (Some((r, g, b)), Some(s)) => [r, g, b].map(|c| (c as u32 >> s).min(255) as u8),
                _ => {
                    let shade = 0.45 + 0.55 * (i as f32 * intensity_scale).min(1.0);
                    class_color(class).map(|c| (c * shade * 255.0).round() as u8)
                }
            };
            cloud.push_lit(layer, [x, y, z], color, ((i as f32 * intensity_scale).min(1.0) * 255.0).round() as u8);
        }
    }
}

/// Which levels of a COPC octree are read for at most `max` points, from the points per level:
/// the number of whole levels, and the next level with the stride that thins it out to fill
/// what is left (when that is at least a tenth of `max`).
fn copc_levels(per_level: &[u64], max: usize) -> (usize, Option<(usize, u64)>) {
    let (mut levels, mut sum) = (0usize, 0u64);
    while levels < per_level.len() && sum + per_level[levels] <= max as u64 {
        sum += per_level[levels];
        levels += 1;
    }
    let left = (max as u64).saturating_sub(sum);
    let partial = (levels < per_level.len() && left > 0 && left >= max as u64 / 10).then(|| (levels, per_level[levels].div_ceil(left).max(1)));
    (levels, partial)
}

/// Reads a cloud-optimised file through its octree, at most `max` points (see [`copc_levels`]).
fn read_copc<R: std::io::Read + std::io::Seek>(mut reader: ::las::CopcReader<'_, R>, stem: &str, max: usize) -> Result<(Vec<Layer>, Vec<(String, String)>), ::las::Error> {
    let header = reader.header().clone();
    let total = header.number_of_points();
    let mut entries: Vec<::las::copc::Entry> = reader.hierarchy_entries().filter(|e| e.point_count > 0 && e.key.l >= 0).collect();
    // by level, then in file order so the reads go forward
    entries.sort_by_key(|e| (e.key.l, e.offset));
    let depth = entries.last().map_or(0, |e| e.key.l as usize + 1);
    let mut per_level = vec![0u64; depth];
    for e in &entries {
        per_level[e.key.l as usize] += e.point_count as u64;
    }
    let (levels, partial) = copc_levels(&per_level, max);
    let mut cloud = Cloud::new(0, 1);
    let b = header.bounds();
    cloud.anchor([b.min.x, b.min.y, b.min.z]);
    let mut colours = Colours::default();
    // counted through the whole thinned level, so that its nodes share the points left
    let mut seen = 0u64;
    for entry in &entries {
        let level = entry.key.l as usize;
        let stride = match partial {
            _ if level < levels => 1,
            Some((l, stride)) if l == level => stride,
            _ => break,
        };
        let data = reader.read_entry(entry)?;
        colours.add(&mut cloud, &data, |_| {
            seen += 1;
            (seen - 1) % stride == 0
        });
    }
    let mut info = cloud.info("LAS", total);
    let shown = cloud.kept();
    if (shown as u64) < total {
        info.push(("Points shown".into(), format!("{shown} (of {total})")));
    }
    let deepest = partial.map_or(levels, |(l, _)| l + 1);
    let thinned = if partial.is_some() { ", the last one thinned out" } else { "" };
    info.push(("COPC octree levels shown".into(), format!("{deepest} of {depth}{thinned}")));
    if let Some(copc) = header.copc_info_vlr() {
        // the spacing halves with every level
        info.push(("COPC point spacing shown".into(), format!("{:.3}", copc.spacing / (1u64 << deepest.saturating_sub(1).min(62)) as f64)));
    }
    Ok(finish(cloud, info, &header, stem))
}

/// The layers of a cloud and the project information about its file.
fn finish(cloud: Cloud, mut info: Vec<(String, String)>, header: &::las::Header, stem: &str) -> (Vec<Layer>, Vec<(String, String)>) {
    if header.copc_info_vlr().is_some() {
        info.push(("Cloud optimised (COPC)".into(), "yes".into()));
    }
    info.push(("LAS version".into(), header.version().to_string()));
    info.push(("Point format".into(), header.point_format().to_string()));
    if let Some(crs) = coordinate_system(header) {
        info.push(("Coordinate system".into(), crs));
    }
    let software = header.generating_software().trim_matches(char::from(0)).trim();
    if !software.is_empty() {
        info.push(("Generating software".into(), software.to_string()));
    }
    let mut layers = cloud.into_layers(true);
    // one layer of unclassified points: named after the file
    if let [layer] = layers.as_mut_slice() {
        if matches!(layer.name.as_str(), "Never classified" | "Unclassified") {
            layer.name = stem.to_string();
        }
    }
    (layers, info)
}

/// The coordinate system: an EPSG code from the GeoTIFF keys, or the name in the WKT.
fn coordinate_system(header: &::las::Header) -> Option<String> {
    if let Ok(Some(crs)) = header.get_geotiff_crs() {
        if let Some(code) = crs.get_projected_crs_geo_key_value().or(crs.get_geodetic_crs_geo_key_value()).filter(|c| *c < 32767) {
            return Some(format!("EPSG:{code}"));
        }
    }
    let wkt = String::from_utf8_lossy(header.get_wkt_crs_bytes()?).into_owned();
    let name = wkt.split('"').nth(1)?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The name of an ASPRS class (LAS 1.4 R15).
pub fn class_name(class: u8) -> String {
    let name = match class {
        0 => "Never classified",
        1 => "Unclassified",
        2 => "Ground",
        3 => "Low vegetation",
        4 => "Medium vegetation",
        5 => "High vegetation",
        6 => "Building",
        7 => "Low point (noise)",
        8 => "Model key point",
        9 => "Water",
        10 => "Rail",
        11 => "Road surface",
        12 => "Overlap",
        13 => "Wire guard",
        14 => "Wire conductor",
        15 => "Transmission tower",
        16 => "Wire-structure connector",
        17 => "Bridge deck",
        18 => "High noise",
        19 => "Overhead structure",
        20 => "Ignored ground",
        21 => "Snow",
        22 => "Temporal exclusion",
        _ => return format!("Class {class}"),
    };
    name.into()
}

/// The colour of an ASPRS class (sRGB 0..1), as point cloud viewers show them.
pub fn class_color(class: u8) -> [f32; 3] {
    match class {
        0 | 1 => [0.78, 0.78, 0.78],
        2 => [0.64, 0.50, 0.34],
        3 => [0.62, 0.80, 0.40],
        4 => [0.38, 0.68, 0.28],
        5 => [0.16, 0.52, 0.18],
        6 => [0.86, 0.38, 0.26],
        7 | 18 => [0.90, 0.22, 0.70],
        8 => [0.95, 0.85, 0.30],
        9 => [0.22, 0.48, 0.88],
        10 => [0.55, 0.38, 0.62],
        11 => [0.48, 0.48, 0.52],
        12 => [0.90, 0.88, 0.45],
        13 | 14 | 16 => [0.96, 0.70, 0.15],
        15 => [0.62, 0.42, 0.22],
        17 => [0.62, 0.62, 0.72],
        19 => [0.72, 0.56, 0.78],
        20 => [0.55, 0.47, 0.40],
        21 => [0.92, 0.95, 0.98],
        _ => {
            // a steady colour per user class
            let h = (class as u32).wrapping_mul(2_654_435_761);
            [(h >> 8) & 0xff, (h >> 16) & 0xff, (h >> 24) & 0xff].map(|c| 0.35 + 0.55 * c as f32 / 255.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::pointcloud::origin_of;
    use crate::loaders::{load_with, Progress, Source};

    /// A small LAS (or LAZ) file: a 20 x 20 grid of ground points round RD New coordinates and a
    /// 10 x 10 block of building points above it, with RGB in 16 bits when `color`.
    fn sample(color: bool, compressed: bool) -> Vec<u8> {
        let mut builder = ::las::Builder::from((1, 4));
        builder.point_format = ::las::point::Format::new(if color { 3 } else { 1 }).unwrap();
        builder.point_format.is_compressed = compressed;
        builder.transforms = ::las::Vector {
            x: ::las::Transform { scale: 0.001, offset: 155_000.0 },
            y: ::las::Transform { scale: 0.001, offset: 463_000.0 },
            z: ::las::Transform { scale: 0.001, offset: 0.0 },
        };
        let header = builder.into_header().unwrap();
        let mut writer = ::las::Writer::new(std::io::Cursor::new(Vec::new()), header).unwrap();
        let mut put = |x: f64, y: f64, z: f64, class: u8, rgb: (u16, u16, u16), intensity: u16| {
            let point = ::las::Point {
                x: 155_000.0 + x,
                y: 463_000.0 + y,
                z,
                intensity,
                classification: ::las::point::Classification::new(class).unwrap(),
                color: color.then_some(::las::Color { red: rgb.0, green: rgb.1, blue: rgb.2 }),
                gps_time: Some(0.0),
                ..Default::default()
            };
            writer.write_point(point).unwrap();
        };
        for i in 0..400 {
            put((i % 20) as f64, (i / 20) as f64, 1.5, 2, (40_000, 30_000, 20_000), 100 + i as u16);
        }
        for i in 0..100 {
            put(5.0 + (i % 10) as f64 * 0.5, 5.0 + (i / 10) as f64 * 0.5, 9.0, 6, (65_535, 0, 0), 500);
        }
        writer.into_inner().unwrap().into_inner()
    }

    fn load_bytes(name: &str, data: Vec<u8>) -> crate::model::Model {
        load_with(&Source::Bytes { name: name.into(), data: Arc::new(data) }, &Progress::default()).expect(name)
    }

    #[test]
    fn reads_classes_colours_and_a_local_origin() {
        let model = load_bytes("site.las", sample(true, false));
        assert_eq!(model.format, "LAS");
        assert_eq!((model.point_count(), model.triangle_count()), (500, 0));
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Ground", "Building"]);
        assert!(model.layers.iter().all(|l| l.points));
        // 16-bit RGB to 8 bits
        assert_eq!(model.layers[1].colors.as_ref().unwrap()[0], [255, 0, 0, 255]);
        assert_eq!(model.layers[0].colors.as_ref().unwrap()[0], [156, 117, 78, 51]); // the fourth byte is its intensity
        // millimetres survive the move to the low corner of the header's bounds
        assert_eq!(origin_of(&model.project.info), Some("155000, 463000, 1"));
        assert_eq!(model.bounds.min, [0.0, 0.0, 0.5]);
        assert_eq!(model.bounds.max, [19.0, 19.0, 8.0]);
        assert!(model.project.info.iter().any(|(k, v)| k == "Points in file" && v == "500"));
    }

    #[test]
    fn colours_by_class_and_intensity_without_rgb() {
        let model = load_bytes("scan.las", sample(false, false));
        let ground = &model.layers[0];
        assert_eq!(ground.color, class_color(2));
        let colors = ground.colors.as_ref().unwrap();
        // brighter with more intensity, in the hue of the class
        assert!(colors[399][0] > colors[0][0]);
        assert!(colors[0][0] > colors[0][2]);
    }

    #[test]
    fn reads_laz_and_thins_out_large_clouds() {
        let laz = load_bytes("site.LAZ", sample(true, true));
        assert_eq!(laz.point_count(), 500);
        let las = load_bytes("site.las", sample(true, false));
        assert_eq!(laz.bounds, las.bounds);
        assert_eq!(laz.layers[1].colors, las.layers[1].colors);

        let reader = ::las::Reader::new(std::io::Cursor::new(sample(true, true))).unwrap();
        let (layers, info) = read(reader, "site", 120).unwrap();
        let kept: usize = layers.iter().map(Layer::vertex_count).sum();
        assert_eq!(kept, 100, "every 5th of 500");
        assert!(info.iter().any(|(k, v)| k == "Points shown" && v == "100 (1 in 5)"));
    }

    /// A small real COPC file (the Autzen sample of the `las` crate's tests, MIT).
    const AUTZEN: &[u8] = include_bytes!("../../test-models/autzen.copc.laz");

    #[test]
    fn reads_a_copc_file_through_its_octree() {
        let model = load_bytes("autzen.copc.laz", AUTZEN.to_vec());
        assert_eq!(model.format, "LAS");
        let info = &model.project.info;
        let get = |key: &str| info.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        assert_eq!(get("Cloud optimised (COPC)"), Some("yes"));
        let total: usize = get("Points in file").unwrap().parse().unwrap();
        // small enough to show whole: every level, every point
        assert_eq!(model.point_count(), total);
        assert!(get("COPC octree levels shown").is_some_and(|v| v.split(" of ").next() == v.split(" of ").nth(1)));
        assert!(model.bounds.is_valid());
        // the same points as the plain LAZ reader finds
        let plain = ::las::Reader::new(std::io::Cursor::new(AUTZEN.to_vec())).unwrap();
        let (layers, _) = read(plain, "autzen", usize::MAX).unwrap();
        assert_eq!(layers.iter().map(Layer::vertex_count).sum::<usize>(), total);
        assert_eq!(model.layers.len(), layers.len());

        // the sample is one node of 107 points: with a limit of 50 it is thinned out
        let reader = ::las::CopcReader::new(std::io::Cursor::new(AUTZEN.to_vec())).unwrap();
        let (layers, info) = read_copc(reader, "autzen", 50).unwrap();
        let kept: usize = layers.iter().map(Layer::vertex_count).sum();
        assert_eq!(kept, total.div_ceil(total.div_ceil(50)), "every n-th of {total}");
        assert!(info.iter().any(|(k, v)| k == "COPC octree levels shown" && v.contains("thinned")), "{info:?}");
    }

    #[test]
    fn copc_levels_fill_the_point_limit() {
        // 100, 800, 6400 points per level
        let levels = [100, 800, 6_400];
        assert_eq!(copc_levels(&levels, 10_000), (3, None));
        assert_eq!(copc_levels(&levels, 900), (2, None));
        // 900 whole, 1100 left of the 6400: every 6th point
        assert_eq!(copc_levels(&levels, 2_000), (2, Some((2, 6))));
        // less than a tenth left: no part of a level
        assert_eq!(copc_levels(&levels, 950), (2, None));
        // even the root is too many: thinned out
        assert_eq!(copc_levels(&levels, 50), (0, Some((0, 2))));
    }

    #[test]
    fn class_names_follow_asprs() {
        assert_eq!(class_name(2), "Ground");
        assert_eq!(class_name(6), "Building");
        assert_eq!(class_name(9), "Water");
        assert_eq!(class_name(77), "Class 77");
        assert_eq!(class_color(77), class_color(77));
    }

    /// `O3V_LAS_SAMPLE=dir cargo test write_sample_files -- --ignored`: writes a larger synthetic
    /// LAS and LAZ (a terrain with a building and trees) for screenshots and timings.
    #[test]
    #[ignore]
    fn write_sample_files() {
        let Some(dir) = std::env::var_os("O3V_LAS_SAMPLE") else { return };
        let dir = std::path::PathBuf::from(dir);
        let n: usize = std::env::var("O3V_LAS_POINTS").ok().and_then(|v| v.parse().ok()).unwrap_or(2_000_000);
        for compressed in [false, true] {
            let mut builder = ::las::Builder::from((1, 4));
            builder.point_format = ::las::point::Format::new(3).unwrap();
            builder.point_format.is_compressed = compressed;
            builder.transforms = ::las::Vector {
                x: ::las::Transform { scale: 0.001, offset: 155_000.0 },
                y: ::las::Transform { scale: 0.001, offset: 463_000.0 },
                z: ::las::Transform { scale: 0.001, offset: 0.0 },
            };
            let path = dir.join(if compressed { "synthetic.laz" } else { "synthetic.las" });
            let mut writer = ::las::Writer::from_path(&path, builder.into_header().unwrap()).unwrap();
            let mut seed = 12345u64;
            let mut rand = || {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                (seed >> 11) as f64 / (1u64 << 53) as f64
            };
            for _ in 0..n {
                let (mut x, mut y) = (rand() * 200.0, rand() * 200.0);
                let ground = 2.0 * (x / 25.0).sin() + 1.5 * (y / 30.0).cos();
                let r = rand();
                let (z, class, rgb) = if (60.0..120.0).contains(&x) && (70.0..110.0).contains(&y) && r < 0.5 {
                    // a building: a flat roof, or a wall on one of its four sides
                    let side = rand();
                    if side >= 0.6 {
                        match (side * 10.0) as u32 % 4 {
                            0 => x = 60.0,
                            1 => x = 120.0,
                            2 => y = 70.0,
                            _ => y = 110.0,
                        }
                    }
                    let z = if side < 0.6 { 14.0 } else { ground + rand() * (14.0 - ground) };
                    (z, 6u8, (52_000u16, 30_000u16, 24_000u16))
                } else if ((x / 23.0).sin() * (y / 17.0).cos()) > 0.75 && r < 0.4 {
                    (ground + 3.0 + rand() * 8.0, 5, (12_000, 36_000, 14_000))
                } else {
                    (ground, 2, (30_000, 26_000, 20_000))
                };
                let point = ::las::Point {
                    x: 155_000.0 + x,
                    y: 463_000.0 + y,
                    z,
                    intensity: (rand() * 4000.0) as u16,
                    classification: ::las::point::Classification::new(class).unwrap(),
                    color: Some(::las::Color { red: rgb.0, green: rgb.1, blue: rgb.2 }),
                    gps_time: Some(0.0),
                    ..Default::default()
                };
                writer.write_point(point).unwrap();
            }
            writer.close().unwrap();
        }
    }
}
