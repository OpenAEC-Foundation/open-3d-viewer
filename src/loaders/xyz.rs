//! ASCII point clouds: XYZ text files (`.xyz`, `.txt`, `.csv`, `.asc`) and Leica PTS (`.pts`).
//! One point per line: X Y Z, then optional colour (R G B in 0..255, 0..1 or 16 bits), intensity
//! and classification columns, separated by spaces, tabs, commas or semicolons (then with decimal
//! commas). A header line naming the columns (`X,Y,Z,R,G,B`, also CloudCompare's `//X,Y,Z`) is
//! followed; without one the colour columns are found from their values. `#` and `//` lines are
//! comments. PTS files start with their point count, and each count line starts a scan, a layer
//! of its own; their columns are X Y Z, intensity, R G B.
//!
//! Points take their colour, else their intensity as grey, else a colour by height. A
//! classification column gives a layer per class in the ASPRS colours, as for LAS. Thinned out
//! evenly to [`MAX_POINTS`](super::pointcloud::MAX_POINTS); far coordinates move to a local
//! origin. A `.txt` or `.csv` file opens only when its lines are columns of numbers.

use super::las::{class_color, class_name};
use super::pointcloud::{color_by_height, grey, intensity_range, Cloud, MAX_POINTS};
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};

pub struct XyzLoader;

impl Loader for XyzLoader {
    fn name(&self) -> &'static str {
        "XYZ"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["xyz", "txt", "csv", "asc"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        load(source, progress, false)
    }
}

pub struct PtsLoader;

impl Loader for PtsLoader {
    fn name(&self) -> &'static str {
        "PTS"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["pts"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        load(source, progress, true)
    }
}

fn load(source: &Source, progress: &Progress, pts: bool) -> Result<Loaded, String> {
    progress.set(Phase::Read);
    let data = source.read()?;
    progress.set(Phase::Convert);
    let (layers, info) = read(&data, &source.stem(), pts, MAX_POINTS).map_err(|e| format!("{}: {e}", source.name()))?;
    Ok(Loaded { layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } })
}

/// Fields read per line; further columns are counted, not kept.
const FIELDS: usize = 24;
/// Lines looked at to find the columns.
const SAMPLE: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Delimiter {
    Whitespace,
    Comma,
    /// Semicolons, with decimal commas allowed.
    Semicolon,
}

/// Splits `line` into `out`, returning the number of fields (also those beyond `out`).
fn split<'a>(line: &'a str, delimiter: Delimiter, out: &mut [&'a str; FIELDS]) -> usize {
    let mut n = 0;
    let mut put = |f: &'a str| {
        if n < FIELDS {
            out[n] = f;
        }
        n += 1;
    };
    match delimiter {
        Delimiter::Whitespace => line.split_ascii_whitespace().for_each(&mut put),
        Delimiter::Comma => line.split(',').map(|f| f.trim().trim_matches('"')).for_each(&mut put),
        Delimiter::Semicolon => line.split(';').map(|f| f.trim().trim_matches('"')).for_each(&mut put),
    }
    n
}

fn number(field: &str, delimiter: Delimiter) -> Option<f64> {
    match field.parse::<f64>() {
        Ok(v) => Some(v),
        Err(_) if delimiter == Delimiter::Semicolon && field.contains(',') => field.replace(',', ".").parse().ok(),
        Err(_) => None,
    }
}

/// The lines of the file, without line ends and a byte order mark.
pub(super) fn lines(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    data.split(|&b| b == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l))
}

/// What a line is, before its numbers are read.
enum Line<'a> {
    Blank,
    /// A `#` or `//` comment, without its marker.
    Comment(&'a str),
    Text(&'a str),
}

fn classify(line: &[u8]) -> Line<'_> {
    let text = match std::str::from_utf8(line) {
        Ok(t) => t.trim(),
        Err(_) => return Line::Comment(""),
    };
    if text.is_empty() {
        Line::Blank
    } else if let Some(rest) = text.strip_prefix("//").or_else(|| text.strip_prefix('#')).or_else(|| text.strip_prefix('%')) {
        Line::Comment(rest)
    } else {
        Line::Text(text)
    }
}

/// What a column holds.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Role {
    X,
    Y,
    Z,
    Red,
    Green,
    Blue,
    Intensity,
    Class,
}

/// The role of a header column name.
fn role(name: &str) -> Option<Role> {
    let name = name.trim().trim_matches(|c| c == '"' || c == '\'').trim_start_matches("//").trim();
    // `X[m]`, `x (m)`, `Scalar field: Intensity`
    let name = name.split(['[', '(']).next().unwrap_or("").trim().to_ascii_lowercase();
    let name = name.rsplit(':').next().unwrap_or("").trim().replace([' ', '-'], "_");
    Some(match name.as_str() {
        "x" | "east" | "easting" | "e" | "x_coord" | "x_coordinate" => Role::X,
        "y" | "north" | "northing" | "n" | "y_coord" | "y_coordinate" => Role::Y,
        "z" | "elev" | "elevation" | "height" | "h" | "alt" | "altitude" | "z_coord" | "z_coordinate" => Role::Z,
        "r" | "red" => Role::Red,
        "g" | "green" => Role::Green,
        "b" | "blue" => Role::Blue,
        "i" | "intensity" | "int" | "scalar_intensity" | "reflectance" | "amplitude" => Role::Intensity,
        "class" | "classification" | "scalar_classification" | "label" => Role::Class,
        _ => return None,
    })
}

/// The range and kind of the values of a column in the sample.
#[derive(Debug, Clone, Copy)]
struct Stats {
    min: f64,
    max: f64,
    integer: bool,
    seen: usize,
}

impl Default for Stats {
    fn default() -> Self {
        Self { min: f64::INFINITY, max: f64::NEG_INFINITY, integer: true, seen: 0 }
    }
}

impl Stats {
    fn add(&mut self, v: f64) {
        self.min = self.min.min(v);
        self.max = self.max.max(v);
        self.integer &= v.fract() == 0.0;
        self.seen += 1;
    }

    /// 8-bit colour values.
    fn bytes(&self) -> bool {
        self.seen > 0 && self.integer && self.min >= 0.0 && self.max <= 255.0
    }

    /// 0..1 colour values.
    fn unit(&self) -> bool {
        self.seen > 0 && !self.integer && self.min >= 0.0 && self.max <= 1.0
    }

    /// 16-bit colour values.
    fn words(&self) -> bool {
        self.seen > 0 && self.integer && self.min >= 0.0 && self.max <= 65535.0
    }
}

/// How the lines of a file are read.
#[derive(Debug)]
struct Layout {
    delimiter: Delimiter,
    xyz: [usize; 3],
    rgb: Option<[usize; 3]>,
    /// Multiplier from the stored colour to 0..255.
    rgb_scale: f64,
    intensity: Option<(usize, (f64, f64))>,
    class: Option<usize>,
    /// The column names for the properties, e.g. `X Y Z Intensity R G B`.
    columns: String,
    /// The first line of text names the columns.
    header_line: bool,
}

/// Finds the delimiter, header and columns from the first lines of the file.
fn layout(data: &[u8], pts: bool) -> Result<Layout, String> {
    let not_a_cloud = "not a point cloud (expected lines of X Y Z numbers)";
    let mut header: Option<Vec<String>> = None;
    let mut header_line = false;
    let mut delimiter = None;
    let mut rows = 0;
    let mut bad = 0;
    let mut widths: Vec<(usize, usize)> = Vec::new();
    let mut stats = [Stats::default(); FIELDS];
    let mut fields = [""; FIELDS];
    let mut content = 0;
    for line in lines(data).take(SAMPLE) {
        let text = match classify(line) {
            Line::Blank => continue,
            Line::Comment(c) => {
                // CloudCompare: `//X,Y,Z,R,G,B`
                if header.is_none() && rows == 0 && c.split([',', ';', ' ', '\t']).filter_map(role).count() >= 3 {
                    header = Some(c.split([',', ';', '\t']).flat_map(|f| f.split_whitespace().next()).map(str::to_string).collect());
                }
                continue;
            }
            Line::Text(t) => t,
        };
        content += 1;
        let d = *delimiter.get_or_insert_with(|| match (text.contains(';'), text.contains(',')) {
            (true, _) => Delimiter::Semicolon,
            (false, true) => Delimiter::Comma,
            _ => Delimiter::Whitespace,
        });
        let n = split(text, d, &mut fields);
        // a count line (PTS, and some XYZ writers)
        if n == 1 && fields[0].parse::<u64>().is_ok() {
            content -= 1;
            continue;
        }
        let numbers = fields[..n.min(FIELDS)].iter().filter(|f| number(f, d).is_some()).count();
        if numbers < n.min(FIELDS) {
            // the first line of text names the columns
            if content == 1 && header.is_none() && rows == 0 {
                header_line = true;
                header = Some(fields[..n.min(FIELDS)].iter().map(|f| f.to_string()).collect());
                if header.as_ref().is_some_and(|h| h.len() == 1) {
                    // a header in another separator than the numbers below it
                    delimiter = None;
                    header = Some(text.split([',', ';', '\t', ' ']).filter(|f| !f.is_empty()).map(str::to_string).collect());
                }
                continue;
            }
            bad += 1;
            if bad > 20 && bad * 5 > rows {
                return Err(not_a_cloud.into());
            }
            continue;
        }
        rows += 1;
        match widths.iter_mut().find(|(w, _)| *w == n) {
            Some((_, count)) => *count += 1,
            None => widths.push((n, 1)),
        }
        for (k, f) in fields[..n.min(FIELDS)].iter().enumerate() {
            if let Some(v) = number(f, d) {
                stats[k].add(v);
            }
        }
    }
    let delimiter = delimiter.unwrap_or(Delimiter::Whitespace);
    // the usual number of columns
    let width = widths.iter().max_by_key(|(_, count)| *count).map(|(w, _)| *w).unwrap_or(0);
    if rows == 0 || bad * 5 > rows || width < 3 {
        return Err(not_a_cloud.into());
    }
    let width = width.min(FIELDS);
    let roles: Vec<Option<Role>> = match &header {
        Some(names) => names.iter().map(|n| role(n)).collect(),
        None => vec![None; width],
    };
    let find = |r: Role| roles.iter().position(|&x| x == Some(r)).filter(|&k| k < width);
    let mut layout = Layout { delimiter, xyz: [0, 1, 2], rgb: None, rgb_scale: 1.0, intensity: None, class: None, columns: String::new(), header_line };
    if let (Some(x), Some(y), Some(z)) = (find(Role::X), find(Role::Y), find(Role::Z)) {
        layout.xyz = [x, y, z];
    } else if let (Some(x), Some(y)) = (find(Role::X), find(Role::Y)) {
        // X, Y and a value column such as a height
        layout.xyz = [x, y, (0..width).find(|k| ![x, y].contains(k) && roles.get(*k).copied().flatten().is_none()).unwrap_or(2)];
    }
    if layout.xyz.iter().any(|&k| stats[k].seen * 5 < rows * 4) {
        return Err(not_a_cloud.into());
    }
    if header.is_some() {
        if let (Some(r), Some(g), Some(b)) = (find(Role::Red), find(Role::Green), find(Role::Blue)) {
            layout.rgb = Some([r, g, b]);
        }
        layout.intensity = find(Role::Intensity).map(|k| (k, (0.0, 0.0)));
        layout.class = find(Role::Class);
    } else {
        // no names: colours where the values are 0..255 (or 0..1, or 16 bits) in three columns
        let triple = |c: usize, test: fn(&Stats) -> bool| c + 3 <= width && (c..c + 3).all(|k| test(&stats[k]));
        let rgb_at = if pts {
            match width {
                3 => None,
                4 | 5 => None,
                6 => Some(3),
                _ => Some(4),
            }
        } else if width >= 7 && triple(4, Stats::bytes) {
            // X Y Z intensity R G B, as scanners write it (also when intensity looks like a colour)
            Some(4)
        } else if width >= 6 && (triple(3, Stats::bytes) || triple(3, Stats::unit) || (width == 6 && triple(3, Stats::words))) {
            Some(3)
        } else {
            (5..width.saturating_sub(2)).find(|&c| triple(c, Stats::bytes))
        };
        layout.rgb = rgb_at.map(|c| [c, c + 1, c + 2]);
        let intensity = match (rgb_at, width) {
            (Some(4), _) | (None, 4..=5) => Some(3),
            _ => None,
        };
        layout.intensity = intensity.map(|k| (k, (0.0, 0.0)));
    }
    if let Some(rgb) = layout.rgb {
        let s: Vec<&Stats> = rgb.iter().map(|&k| &stats[k]).collect();
        layout.rgb_scale = if s.iter().all(|s| s.unit() || (s.max <= 1.0 && s.min >= 0.0)) && !s.iter().all(|s| s.integer) {
            255.0
        } else if s.iter().any(|s| s.max > 255.0) && s.iter().all(|s| s.words()) {
            1.0 / 257.0
        } else {
            1.0
        };
    }
    if let Some((k, range)) = layout.intensity.as_mut() {
        *range = intensity_range(stats[*k].min, stats[*k].max);
    }
    let names: Vec<String> = (0..width)
        .map(|k| {
            let role = if layout.xyz[0] == k {
                "X"
            } else if layout.xyz[1] == k {
                "Y"
            } else if layout.xyz[2] == k {
                "Z"
            } else if layout.rgb.is_some_and(|c| c[0] == k) {
                "R"
            } else if layout.rgb.is_some_and(|c| c[1] == k) {
                "G"
            } else if layout.rgb.is_some_and(|c| c[2] == k) {
                "B"
            } else if layout.intensity.is_some_and(|(i, _)| i == k) {
                "Intensity"
            } else if layout.class == Some(k) {
                "Class"
            } else {
                "-"
            };
            match header.as_ref().and_then(|h| h.get(k)) {
                Some(name) if role == "-" => format!("{name} (not read)"),
                _ => role.to_string(),
            }
        })
        .collect();
    layout.columns = names.join(" ");
    Ok(layout)
}

/// Reads the points of an ASCII cloud (keeping at most `max`): the layers and the project
/// information.
fn read(data: &[u8], stem: &str, pts: bool, max: usize) -> Result<(Vec<Layer>, Vec<(String, String)>), String> {
    let layout = layout(data, pts)?;
    // a line per point: the number of lines bounds the number of points
    let estimate = data.iter().filter(|&&b| b == b'\n').count() as u64 + 1;
    let mut cloud = Cloud::new(estimate, max);
    let mut fields = [""; FIELDS];
    let mut total = 0u64;
    let mut scan = 0u32;
    let mut scans = 0u32;
    let d = layout.delimiter;
    let colored = layout.rgb.is_some() || layout.intensity.is_some();
    let mut skip_header = layout.header_line;
    for line in lines(data) {
        let Line::Text(text) = classify(line) else { continue };
        if std::mem::take(&mut skip_header) {
            continue;
        }
        // a count line: in PTS the start of a scan
        if !text.contains([' ', '\t', ',', ';']) {
            if pts && text.parse::<u64>().is_ok() {
                scans += 1;
                scan = scans;
            }
            continue;
        }
        if !cloud.keep() {
            total += 1;
            continue;
        }
        let n = split(text, d, &mut fields).min(FIELDS);
        let get = |k: usize| if k < n { number(fields[k], d) } else { None };
        let (Some(x), Some(y), Some(z)) = (get(layout.xyz[0]), get(layout.xyz[1]), get(layout.xyz[2])) else {
            // the header line, or a line of text
            continue;
        };
        total += 1;
        let color = match (layout.rgb, layout.intensity) {
            (Some(c), _) => c.map(|k| (get(k).unwrap_or(0.0) * layout.rgb_scale).round().clamp(0.0, 255.0) as u8),
            (None, Some((k, range))) => grey(get(k).unwrap_or(range.0), range),
            (None, None) => [0; 3],
        };
        let layer = match layout.class.and_then(get) {
            Some(class) => {
                let class = class.clamp(0.0, 255.0) as u8;
                cloud.layer(CLASS_KEYS + class as u32, || Layer { name: class_name(class), color: class_color(class), ..Layer::default() })
            }
            None => cloud.layer(scan, || Layer {
                name: if scan == 0 { stem.to_string() } else { format!("Scan {scan}") },
                color: SCAN_COLOR,
                ..Layer::default()
            }),
        };
        cloud.push(layer, [x, y, z], color);
    }
    if total == 0 {
        return Err("no points".into());
    }
    let format = if pts { "PTS" } else { "XYZ" };
    let mut info = cloud.info(format, total);
    let mut layers = cloud.into_layers(true);
    if !colored {
        color_by_height(&mut layers);
    }
    // one scan: named after the file
    if let [layer] = layers.as_mut_slice() {
        if scans == 1 && layer.name == "Scan 1" {
            layer.name = stem.to_string();
        }
    }
    if scans > 1 {
        info.insert(0, ("Scans".into(), scans.to_string()));
    }
    info.push(("Columns".into(), layout.columns));
    if !colored {
        info.push(("Point colours".into(), "By height (the file has no colours)".into()));
    }
    Ok((layers, info))
}

/// Layer keys of classes, after those of scans.
const CLASS_KEYS: u32 = 1 << 20;

/// The layer colour of a scan (the points take their own colours).
const SCAN_COLOR: [f32; 3] = [0.80, 0.80, 0.82];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::pointcloud::origin_of;
    use crate::loaders::{load_with, Progress, Source};
    use std::sync::Arc;

    fn load(name: &str, text: &str) -> Result<crate::model::Model, String> {
        load_with(&Source::Bytes { name: name.into(), data: Arc::new(text.as_bytes().to_vec()) }, &Progress::default())
    }

    #[test]
    fn reads_xyz_with_colours_found_from_their_values() {
        // RD New coordinates, X Y Z R G B separated by spaces, a comment
        let model = load("scan.xyz", "# exported\n155000.001 463000.000 1.5 255 0 0\n155001 463000 1.5 0 255 0\n\n155000.5 463000.5 9 0 0 255\n").unwrap();
        assert_eq!(model.format, "XYZ");
        assert_eq!((model.point_count(), model.triangle_count()), (3, 0));
        let layer = &model.layers[0];
        assert_eq!(layer.name, "scan");
        assert!(layer.points);
        assert_eq!(layer.colors.as_ref().unwrap()[2], [0, 0, 255, 255]);
        assert_eq!(origin_of(&model.project.info), Some("155000, 463000, 1"));
        assert!((layer.positions[0] - 0.001).abs() < 1e-6);
        assert!(model.project.info.iter().any(|(k, v)| k == "Columns" && v == "X Y Z R G B"));
    }

    #[test]
    fn follows_a_csv_header_and_classes() {
        let text = "id,Easting,Northing,Elevation,Intensity,Classification\n1,0,0,0,0,2\n2,1,0,0,200,2\n3,1,1,5,400,6\n";
        let model = load("survey.CSV", text).unwrap();
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Ground", "Building"]);
        let ground = &model.layers[0];
        assert_eq!(ground.positions, [0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        // intensity 0..400 of a 12-bit range... grey from the range found in the file
        let c = ground.colors.as_ref().unwrap();
        assert!(c[1][0] > c[0][0]);
        assert_eq!(model.layers[1].positions, [1.0, 1.0, 5.0]);
        assert!(model.project.info.iter().any(|(_, v)| v == "id (not read) X Y Z Intensity Class"));
        // CloudCompare's header comment, semicolons and decimal commas
        let model = load("cc.txt", "//X;Y;Z;R;G;B\n1,5;2;3;10;20;30\n2;2;3;10;20;30\n").unwrap();
        assert_eq!(&model.layers[0].positions[..3], &[1.5, 2.0, 3.0]);
        assert_eq!(model.layers[0].colors.as_ref().unwrap()[0], [10, 20, 30, 255]);
    }

    #[test]
    fn plain_xyz_is_coloured_by_height_and_intensity_is_grey() {
        let model = load("t.asc", "0\t0\t0\n1\t0\t1\n0\t1\t2\n").unwrap();
        let c = model.layers[0].colors.as_ref().unwrap();
        assert!(c[0][2] > c[0][0] && c[2][0] > c[2][2], "blue low, red high: {c:?}");
        let model = load("t.xyz", "0 0 0 0.0\n1 0 0 1.0\n").unwrap();
        assert_eq!(model.layers[0].colors.as_ref().unwrap(), &[[0, 0, 0, 255], [255, 255, 255, 255]]);
    }

    #[test]
    fn txt_and_csv_must_be_columns_of_numbers() {
        assert!(load("notes.txt", "Dear all,\nthe model is attached.\nRegards\n").unwrap_err().contains("not a point cloud"));
        assert!(load("table.csv", "name,amount\nwall,3\nslab,4\n").unwrap_err().contains("not a point cloud"));
        // a molecule (chemistry XYZ): element symbols in the first column
        assert!(load("water.xyz", "3\nwater\nO 0 0 0\nH 0.76 0.59 0\nH -0.76 0.59 0\n").is_err());
        assert!(load("two.csv", "1,2\n3,4\n").is_err());
    }

    #[test]
    fn pts_scans_with_intensity_and_colour() {
        // two scans, X Y Z I R G B, Leica's intensity range -2048..2047
        let text = "2\n0 0 0 -2048 255 0 0\n1 0 0 2047 255 0 0\n1\n5 5 5 0 0 0 255\n";
        let model = load("site.pts", text).unwrap();
        assert_eq!(model.format, "PTS");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Scan 1", "Scan 2"]);
        assert_eq!(model.layers[1].colors.as_ref().unwrap()[0], [0, 0, 255, 255]);
        assert!(model.project.info.iter().any(|(k, v)| k == "Scans" && v == "2"));
        // intensity only
        let model = load("i.pts", "2\n0 0 0 -2048\n1 1 1 2047\n").unwrap();
        assert_eq!(model.layers[0].name, "i");
        assert_eq!(model.layers[0].colors.as_ref().unwrap(), &[[0, 0, 0, 255], [255, 255, 255, 255]]);
    }

    #[test]
    fn reads_the_repository_sample() {
        let model = crate::loaders::load(&std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/test-pointcloud.xyz")).unwrap();
        assert_eq!((model.format.as_str(), model.point_count()), ("XYZ", 16));
    }

    #[test]
    fn thins_out_large_files() {
        let text: String = (0..1000).map(|i| format!("{i} 0 0\n")).collect();
        let (layers, info) = read(text.as_bytes(), "big", false, 100).unwrap();
        let kept: usize = layers.iter().map(Layer::vertex_count).sum();
        assert!((90..=100).contains(&kept), "{kept}");
        assert!(info.iter().any(|(k, v)| k == "Points in file" && v == "1000"));
        assert!(info.iter().any(|(k, _)| k == "Points shown"));
    }
}
