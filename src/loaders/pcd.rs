//! Point Cloud Library files (`.pcd`, versions 0.5 to 0.7): ASCII, binary and
//! `binary_compressed` (LZF) data. X Y Z with a packed `rgb` / `rgba` colour, separate `r g b`
//! fields, or else an `intensity` as grey; points without colour or intensity are coloured by
//! height. One layer; NaN points (of organised clouds) are left out. The `VIEWPOINT` is shown in
//! the properties, not applied (as PCL does). Thinned out evenly to
//! [`MAX_POINTS`](super::pointcloud::MAX_POINTS); far coordinates move to a local origin.

use super::pointcloud::{color_by_height, grey, intensity_range, Cloud, Scalar, MAX_POINTS};
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};

pub struct PcdLoader;

impl Loader for PcdLoader {
    fn name(&self) -> &'static str {
        "PCD"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["pcd"]
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

/// A field of the point record.
#[derive(Debug, Clone)]
struct Field {
    name: String,
    scalar: Scalar,
    count: usize,
    /// Byte offset in a binary record.
    offset: usize,
    /// Value index in an ASCII line.
    column: usize,
}

#[derive(Debug, PartialEq)]
enum Data {
    Ascii,
    Binary,
    Compressed,
}

struct Header {
    fields: Vec<Field>,
    points: u64,
    data: Data,
    viewpoint: Option<String>,
    /// Where the data starts.
    body: usize,
    version: String,
}

fn scalar(kind: &str, size: usize) -> Option<Scalar> {
    Some(match (kind, size) {
        ("F", 4) => Scalar::F32,
        ("F", 8) => Scalar::F64,
        ("I", 1) => Scalar::I8,
        ("I", 2) => Scalar::I16,
        ("I", 4) => Scalar::I32,
        ("I", 8) => Scalar::I64,
        ("U", 1) => Scalar::U8,
        ("U", 2) => Scalar::U16,
        ("U", 4) => Scalar::U32,
        ("U", 8) => Scalar::U64,
        _ => return None,
    })
}

fn header(data: &[u8]) -> Result<Header, String> {
    let mut at = 0;
    let (mut names, mut sizes, mut types, mut counts) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut width, mut height, mut points) = (None::<u64>, 1u64, None::<u64>);
    let mut viewpoint = None;
    let mut version = String::new();
    loop {
        let end = data[at..].iter().position(|&b| b == b'\n').map_or(data.len(), |p| at + p + 1);
        if at >= data.len() {
            return Err("no DATA line".into());
        }
        let line = String::from_utf8_lossy(&data[at..end]);
        at = end;
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some((&key, values)) = words.split_first() else { continue };
        let numbers = || values.iter().map(|v| v.parse::<usize>().map_err(|_| format!("bad {key} line"))).collect::<Result<Vec<_>, _>>();
        match key.to_ascii_uppercase().as_str() {
            k if k.starts_with('#') => {}
            "VERSION" => version = values.join(" "),
            "FIELDS" | "COLUMNS" => names = values.iter().map(|v| v.to_string()).collect(),
            "SIZE" => sizes = numbers()?,
            "TYPE" => types = values.iter().map(|v| v.to_ascii_uppercase()).collect(),
            "COUNT" => counts = numbers()?,
            "WIDTH" => width = values.first().and_then(|v| v.parse().ok()),
            "HEIGHT" => height = values.first().and_then(|v| v.parse().ok()).unwrap_or(1),
            "POINTS" => points = values.first().and_then(|v| v.parse().ok()),
            "VIEWPOINT" => viewpoint = Some(values.join(" ")),
            "DATA" => {
                let data = match values.first().map(|v| v.to_ascii_lowercase()).as_deref() {
                    Some("ascii") => Data::Ascii,
                    Some("binary") => Data::Binary,
                    Some("binary_compressed") => Data::Compressed,
                    other => return Err(format!("unknown DATA {}", other.unwrap_or(""))),
                };
                if names.is_empty() {
                    return Err("no FIELDS".into());
                }
                // version 0.5 has no SIZE and TYPE: ASCII floats
                let ascii = data == Data::Ascii;
                if sizes.is_empty() && ascii {
                    sizes = vec![4; names.len()];
                }
                if types.is_empty() && ascii {
                    types = vec!["F".to_string(); names.len()];
                }
                if counts.is_empty() {
                    counts = vec![1; names.len()];
                }
                if sizes.len() != names.len() || types.len() != names.len() || counts.len() != names.len() {
                    return Err("FIELDS, SIZE, TYPE and COUNT differ in length".into());
                }
                let (mut offset, mut column) = (0, 0);
                let mut fields = Vec::new();
                for k in 0..names.len() {
                    let scalar = scalar(&types[k], sizes[k]).ok_or_else(|| format!("unknown type {} {}", types[k], sizes[k]))?;
                    fields.push(Field { name: names[k].to_ascii_lowercase(), scalar, count: counts[k], offset, column });
                    offset += sizes[k] * counts[k];
                    column += counts[k];
                }
                let points = points.or(width.map(|w| w * height)).ok_or("no POINTS")?;
                return Ok(Header { fields, points, data, viewpoint, body: at, version });
            }
            _ => {}
        }
    }
}

/// LZF decompression (liblzf, as PCL writes `binary_compressed`).
fn lzf(input: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let bad = || "corrupt binary_compressed data".to_string();
    let mut out = Vec::with_capacity(size);
    let mut i = 0;
    while i < input.len() {
        let ctrl = input[i] as usize;
        i += 1;
        if ctrl < 32 {
            let run = input.get(i..i + ctrl + 1).ok_or_else(bad)?;
            out.extend_from_slice(run);
            i += ctrl + 1;
        } else {
            let mut len = ctrl >> 5;
            if len == 7 {
                len += *input.get(i).ok_or_else(bad)? as usize;
                i += 1;
            }
            let back = ((ctrl & 0x1f) << 8) + *input.get(i).ok_or_else(bad)? as usize + 1;
            i += 1;
            let start = out.len().checked_sub(back).ok_or_else(bad)?;
            for k in 0..len + 2 {
                let b = out[start + k];
                out.push(b);
            }
        }
        if out.len() > size {
            return Err(bad());
        }
    }
    Ok(out)
}

/// Reads the points (keeping at most `max`).
fn read(data: &[u8], stem: &str, max: usize) -> Result<(Vec<Layer>, Vec<(String, String)>), String> {
    let h = header(data)?;
    let find = |names: &[&str]| h.fields.iter().find(|f| names.contains(&f.name.as_str())).cloned();
    let (Some(fx), Some(fy), Some(fz)) = (find(&["x"]), find(&["y"]), find(&["z"])) else {
        return Err("no x, y and z fields".into());
    };
    let packed = find(&["rgb", "rgba"]);
    let split = match (find(&["r", "red"]), find(&["g", "green"]), find(&["b", "blue"])) {
        (Some(r), Some(g), Some(b)) => Some([r, g, b]),
        _ => None,
    };
    let intensity = find(&["intensity", "i"]);
    let total = h.points;
    let mut cloud = Cloud::new(total, max);
    let layer = cloud.layer(0, || Layer { name: stem.to_string(), color: [0.80, 0.80, 0.82], ..Layer::default() });
    let body = &data[h.body.min(data.len())..];
    let point_size: usize = h.fields.iter().map(|f| f.scalar.size() * f.count).sum();
    // values of point `i`: from a record (binary), a column-major block (compressed) or a line
    let decompressed;
    let (records, column_major): (&[u8], bool) = match h.data {
        Data::Ascii => (&[], false),
        Data::Binary => (body, false),
        Data::Compressed => {
            let word = |k: usize| body.get(4 * k..4 * k + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
            let (Some(packed_size), Some(size)) = (word(0), word(1)) else { return Err("truncated binary_compressed data".into()) };
            if size < point_size * total as usize {
                return Err("binary_compressed data smaller than its points".into());
            }
            decompressed = lzf(body.get(8..8 + packed_size).ok_or("truncated binary_compressed data")?, size)?;
            (&decompressed, true)
        }
    };
    let n = total as usize;
    let value = |f: &Field, i: usize, line: &[f64]| -> f64 {
        match h.data {
            Data::Ascii => line.get(f.column).copied().unwrap_or(f64::NAN),
            _ => {
                // column-major: all values of a field together
                let at = if column_major { f.offset * n + i * f.scalar.size() * f.count } else { i * point_size + f.offset };
                records.get(at..at + f.scalar.size()).map_or(f64::NAN, |b| f.scalar.get(b, false))
            }
        }
    };
    // a packed colour: the bits of a float or an unsigned integer, 0x00RRGGBB
    let unpack = |f: &Field, i: usize, line: &[f64], text: Option<&str>| -> [u8; 3] {
        let bits = match (h.data == Data::Ascii, f.scalar) {
            (true, Scalar::F32) => text.and_then(|t| t.parse::<f32>().ok()).map_or(0, f32::to_bits),
            (false, Scalar::F32) => {
                let at = if column_major { f.offset * n + i * 4 } else { i * point_size + f.offset };
                records.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            }
            _ => value(f, i, line) as u32,
        };
        [(bits >> 16) as u8, (bits >> 8) as u8, bits as u8]
    };
    // intensity: the range from the first points
    let mut range = None;
    let mut sample_range = |first: &mut dyn FnMut(usize) -> f64| {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for i in 0..n.min(10_000) {
            let v = first(i);
            if v.is_finite() {
                (lo, hi) = (lo.min(v), hi.max(v));
            }
        }
        range = Some(if lo.is_finite() { intensity_range(lo, hi) } else { (0.0, 1.0) });
    };
    let mut kept_any = false;
    let mut point = |cloud: &mut Cloud, p: [f64; 3], color: [u8; 3]| {
        kept_any = true;
        cloud.push(layer, p, color);
    };
    let colored = packed.is_some() || split.is_some() || intensity.is_some();
    match h.data {
        Data::Ascii => {
            let text = String::from_utf8_lossy(body);
            let rows: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(n).collect();
            if let Some(f) = &intensity {
                sample_range(&mut |i| rows.get(i).and_then(|l| l.split_ascii_whitespace().nth(f.column)).and_then(|v| v.parse().ok()).unwrap_or(f64::NAN));
            }
            let mut line = Vec::new();
            for row in rows {
                if !cloud.keep() {
                    continue;
                }
                line.clear();
                line.extend(row.split_ascii_whitespace().map(|v| v.parse::<f64>().unwrap_or(f64::NAN)));
                let p = [fx.column, fy.column, fz.column].map(|c| line.get(c).copied().unwrap_or(f64::NAN));
                let color = if let Some(f) = &packed {
                    unpack(f, 0, &line, row.split_ascii_whitespace().nth(f.column))
                } else if let Some(c) = &split {
                    c.clone().map(|f| channel(&f, line.get(f.column).copied().unwrap_or(0.0)))
                } else if let (Some(f), Some(r)) = (&intensity, range) {
                    grey(line.get(f.column).copied().unwrap_or(0.0), r)
                } else {
                    [0; 3]
                };
                point(&mut cloud, p, color);
            }
        }
        _ => {
            if records.len() < point_size * n {
                return Err(format!("the data holds fewer than its {n} points"));
            }
            if let Some(f) = &intensity {
                sample_range(&mut |i| value(f, i, &[]));
            }
            for i in 0..n {
                if !cloud.keep() {
                    continue;
                }
                let p = [value(&fx, i, &[]), value(&fy, i, &[]), value(&fz, i, &[])];
                let color = if let Some(f) = &packed {
                    unpack(f, i, &[], None)
                } else if let Some(c) = &split {
                    c.clone().map(|f| channel(&f, value(&f, i, &[])))
                } else if let (Some(f), Some(r)) = (&intensity, range) {
                    grey(value(f, i, &[]), r)
                } else {
                    [0; 3]
                };
                point(&mut cloud, p, color);
            }
        }
    }
    if !kept_any {
        return Err("no points".into());
    }
    let mut info = cloud.info("PCD", total);
    let kept = cloud.kept();
    let mut layers = cloud.into_layers(false);
    if !colored {
        color_by_height(&mut layers);
    }
    if kept == 0 || layers.iter().all(|l| l.positions.is_empty()) {
        return Err("no valid points (all NaN)".into());
    }
    let fields: Vec<&str> = h.fields.iter().map(|f| f.name.as_str()).collect();
    info.push(("PCD fields".into(), fields.join(" ")));
    info.push(("PCD data".into(), format!("{:?}", h.data).to_ascii_lowercase()));
    if !h.version.is_empty() {
        info.push(("PCD version".into(), h.version.clone()));
    }
    if let Some(v) = h.viewpoint.filter(|v| v.split_whitespace().map(|w| w.parse::<f64>().unwrap_or(0.0)).ne([0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0])) {
        info.push(("Viewpoint (not applied)".into(), v));
    }
    if !colored {
        info.push(("Point colours".into(), "By height (the file has no colours)".into()));
    }
    Ok((layers, info))
}

/// A colour channel: 0..1 floats or 0..255 integers.
fn channel(f: &Field, v: f64) -> u8 {
    let v = if f.scalar.is_float() && v <= 1.0 { v * 255.0 } else { v };
    v.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::{load_with, Progress, Source};
    use std::sync::Arc;

    const HEADER: &str = "# .PCD v0.7 - Point Cloud Data file format\nVERSION 0.7\nFIELDS x y z rgb\nSIZE 4 4 4 4\nTYPE F F F F\nCOUNT 1 1 1 1\nWIDTH 3\nHEIGHT 1\nVIEWPOINT 0 0 0 1 0 0 0\nPOINTS 3\n";

    fn rgb_float(r: u8, g: u8, b: u8) -> f32 {
        f32::from_bits(((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    /// Three points: red at the origin, green at x = 1, NaN (left out).
    fn points() -> Vec<([f32; 3], f32)> {
        vec![([0.0, 0.0, 0.0], rgb_float(255, 0, 0)), ([1.0, 0.0, 0.5], rgb_float(0, 255, 0)), ([f32::NAN; 3], rgb_float(0, 0, 255))]
    }

    fn load(data: Vec<u8>) -> crate::model::Model {
        load_with(&Source::Bytes { name: "cloud.pcd".into(), data: Arc::new(data) }, &Progress::default()).expect("PCD")
    }

    fn check(model: &crate::model::Model) {
        assert_eq!(model.format, "PCD");
        assert_eq!(model.point_count(), 2);
        let layer = &model.layers[0];
        assert_eq!(layer.name, "cloud");
        assert_eq!(layer.positions, [0.0, 0.0, 0.0, 1.0, 0.0, 0.5]);
        assert_eq!(layer.colors.as_ref().unwrap(), &[[255, 0, 0, 255], [0, 255, 0, 255]]);
    }

    #[test]
    fn reads_ascii_with_packed_colours() {
        let mut text = format!("{HEADER}DATA ascii\n");
        for (p, c) in points() {
            text += &format!("{} {} {} {:e}\n", p[0], p[1], p[2], c);
        }
        check(&load(text.into_bytes()));
    }

    #[test]
    fn reads_binary() {
        let mut data = format!("{HEADER}DATA binary\n").into_bytes();
        for (p, c) in points() {
            for v in [p[0], p[1], p[2], c] {
                data.extend_from_slice(&v.to_le_bytes());
            }
        }
        check(&load(data));
    }

    #[test]
    fn reads_binary_compressed() {
        // column-major (all x, all y, all z, all rgb), LZF with literal runs and a back reference
        let mut raw = Vec::new();
        for k in 0..4 {
            for (p, c) in points() {
                let v = if k < 3 { p[k] } else { c };
                raw.extend_from_slice(&v.to_le_bytes());
            }
        }
        // the first 8 bytes as a literal run, then the rest as literals in runs of up to 32
        let mut packed = Vec::new();
        for chunk in raw.chunks(32) {
            packed.push(chunk.len() as u8 - 1);
            packed.extend_from_slice(chunk);
        }
        let mut data = format!("{HEADER}DATA binary_compressed\n").into_bytes();
        data.extend_from_slice(&(packed.len() as u32).to_le_bytes());
        data.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        data.extend_from_slice(&packed);
        check(&load(data));
        // a back reference: "abc" then copy 4 bytes from 3 back = "abcabca"
        assert_eq!(lzf(&[2, b'a', b'b', b'c', 2 << 5, 2], 7).unwrap(), b"abcabca");
    }

    #[test]
    fn intensity_as_grey_and_height_colours_without() {
        let text = "VERSION .7\nFIELDS x y z intensity\nSIZE 4 4 4 4\nTYPE F F F F\nCOUNT 1 1 1 1\nWIDTH 2\nHEIGHT 1\nPOINTS 2\nDATA ascii\n0 0 0 0\n1 1 1 1\n";
        let model = load(text.as_bytes().to_vec());
        assert_eq!(model.layers[0].colors.as_ref().unwrap(), &[[0, 0, 0, 255], [255, 255, 255, 255]]);
        let text = "VERSION .7\nFIELDS x y z\nSIZE 8 8 8\nTYPE F F F\nCOUNT 1 1 1\nWIDTH 2\nHEIGHT 1\nPOINTS 2\nDATA ascii\n0 0 0\n1 1 1\n";
        let model = load(text.as_bytes().to_vec());
        let colors = model.layers[0].colors.as_ref().unwrap();
        assert_ne!(colors[0], colors[1]);
        assert!(model.project.info.iter().any(|(_, v)| v.contains("By height")));
    }
}
