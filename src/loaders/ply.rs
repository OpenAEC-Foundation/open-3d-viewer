//! Adapter from the independent ply-reader file reader to the app's scene model. A PLY file
//! without faces is a point cloud (as scanners, photogrammetry and CloudCompare write them): its
//! vertices become points, read here with full precision, in their colour, else their intensity
//! as grey, else a colour by height; a classification gives a layer per class as for LAS.
//! Thinned out evenly to [`MAX_POINTS`](super::pointcloud::MAX_POINTS); far coordinates move to
//! a local origin.
use super::las::{class_color, class_name};
use super::pointcloud::{color_by_height, grey, intensity_range, Cloud, Scalar, MAX_POINTS};
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};

pub struct PlyLoader;

impl Loader for PlyLoader {
    fn name(&self) -> &'static str { "PLY" }
    fn extensions(&self) -> &'static [&'static str] { &["ply"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        let header = header(&data)?;
        if !header.elements.iter().any(|e| e.name == "face" && e.count > 0) {
            let (layers, info) = points(&data, &header, &source.stem(), MAX_POINTS).map_err(|e| format!("{}: {e}", source.name()))?;
            return Ok(Loaded { layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } });
        }
        let mesh = ply_reader::read(&data)?;
        let layers = vec![Layer { name: source.stem(), color: mesh.color, positions: mesh.positions,
            normals: mesh.normals, colors: mesh.colors, indices: mesh.indices, ..Layer::default() }];
        let project = ProjectData::default();
        Ok(Loaded { layers, views: Vec::new(), project })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Encoding {
    Ascii,
    Little,
    Big,
}

struct Property {
    name: String,
    /// The count type of a list property.
    list: Option<Scalar>,
    scalar: Scalar,
}

struct Element {
    name: String,
    count: u64,
    properties: Vec<Property>,
}

struct Header {
    encoding: Encoding,
    elements: Vec<Element>,
    /// Where the data starts.
    body: usize,
}

fn scalar(name: &str) -> Option<Scalar> {
    Some(match name {
        "char" | "int8" => Scalar::I8,
        "uchar" | "uint8" => Scalar::U8,
        "short" | "int16" => Scalar::I16,
        "ushort" | "uint16" => Scalar::U16,
        "int" | "int32" => Scalar::I32,
        "uint" | "uint32" => Scalar::U32,
        "float" | "float32" => Scalar::F32,
        "double" | "float64" => Scalar::F64,
        _ => return None,
    })
}

fn header(data: &[u8]) -> Result<Header, String> {
    if !data.starts_with(b"ply") {
        return Err("PLY: no ply header".into());
    }
    let end = data.windows(10).position(|w| w == b"end_header").ok_or("PLY: no end_header")?;
    let body = data[end..].iter().position(|&b| b == b'\n').map_or(data.len(), |p| end + p + 1);
    let mut encoding = None;
    let mut elements: Vec<Element> = Vec::new();
    for line in String::from_utf8_lossy(&data[..end]).lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["format", f, ..] => {
                encoding = Some(match *f {
                    "ascii" => Encoding::Ascii,
                    "binary_little_endian" => Encoding::Little,
                    "binary_big_endian" => Encoding::Big,
                    other => return Err(format!("PLY: unknown format {other}")),
                })
            }
            ["element", name, count] => {
                elements.push(Element { name: name.to_string(), count: count.parse().map_err(|_| "PLY: bad element count")?, properties: Vec::new() })
            }
            ["property", "list", c, t, name] => elements.last_mut().ok_or("PLY: property before element")?.properties.push(Property {
                name: name.to_string(),
                list: Some(scalar(c).ok_or("PLY: bad list type")?),
                scalar: scalar(t).ok_or("PLY: bad type")?,
            }),
            ["property", t, name] => elements.last_mut().ok_or("PLY: property before element")?.properties.push(Property {
                name: name.to_string(),
                list: None,
                scalar: scalar(t).ok_or("PLY: bad type")?,
            }),
            _ => {}
        }
    }
    Ok(Header { encoding: encoding.ok_or("PLY: missing format line")?, elements, body })
}

/// Reads the values of the records one after the other, ASCII or binary.
struct Values<'a> {
    data: &'a [u8],
    at: usize,
    encoding: Encoding,
}

impl Values<'_> {
    fn next(&mut self, scalar: Scalar) -> Result<f64, String> {
        let end = "unexpected end of file";
        if self.encoding == Encoding::Ascii {
            let data = self.data;
            while self.at < data.len() && data[self.at].is_ascii_whitespace() {
                self.at += 1;
            }
            let start = self.at;
            while self.at < data.len() && !data[self.at].is_ascii_whitespace() {
                self.at += 1;
            }
            let word = std::str::from_utf8(&data[start..self.at]).map_err(|_| "bad text")?;
            return if word.is_empty() { Err(end.into()) } else { word.parse().map_err(|_| format!("bad number {word:?}")) };
        }
        let b = self.data.get(self.at..self.at + scalar.size()).ok_or(end)?;
        self.at += scalar.size();
        Ok(scalar.get(b, self.encoding == Encoding::Big))
    }

    /// Reads one record of `element` into `out` (list properties are skipped and give NaN).
    fn record(&mut self, element: &Element, out: &mut [f64]) -> Result<(), String> {
        for (k, p) in element.properties.iter().enumerate() {
            match p.list {
                None => out[k] = self.next(p.scalar)?,
                Some(count) => {
                    let n = self.next(count)? as usize;
                    if self.encoding == Encoding::Ascii {
                        for _ in 0..n {
                            self.next(p.scalar)?;
                        }
                    } else {
                        self.at += n * p.scalar.size();
                    }
                    out[k] = f64::NAN;
                }
            }
        }
        Ok(())
    }
}

/// Reads the vertices of a PLY file without faces as a point cloud (keeping at most `max`).
fn points(data: &[u8], h: &Header, stem: &str, max: usize) -> Result<(Vec<Layer>, Vec<(String, String)>), String> {
    let mut values = Values { data, at: h.body, encoding: h.encoding };
    let mut skip = Vec::new();
    for element in &h.elements {
        if element.name == "vertex" {
            break;
        }
        // elements before the vertices (rare): read over them
        skip.resize(element.properties.len(), 0.0);
        for _ in 0..element.count {
            values.record(element, &mut skip)?;
        }
    }
    let vertex = h.elements.iter().find(|e| e.name == "vertex").ok_or("no vertex element")?;
    let find = |names: &[&str]| vertex.properties.iter().position(|p| names.contains(&p.name.to_ascii_lowercase().as_str()));
    let (Some(x), Some(y), Some(z)) = (find(&["x"]), find(&["y"]), find(&["z"])) else { return Err("no x, y and z properties".into()) };
    let rgb = match (find(&["red", "r", "diffuse_red"]), find(&["green", "g", "diffuse_green"]), find(&["blue", "b", "diffuse_blue"])) {
        (Some(r), Some(g), Some(b)) => Some([r, g, b]),
        _ => None,
    };
    let intensity = find(&["intensity", "scalar_intensity", "i", "reflectance", "scalar_reflectance"]);
    let class = find(&["classification", "scalar_classification", "class", "scalar_class"]);
    // colours: 0..1 floats, 16 bits or 8 bits
    let rgb_scale = match rgb.map(|c| vertex.properties[c[0]].scalar) {
        Some(Scalar::F32 | Scalar::F64) => 255.0,
        Some(Scalar::U16 | Scalar::I16) => 1.0 / 257.0,
        _ => 1.0,
    };
    let total = vertex.count;
    let mut cloud = Cloud::new(total, max);
    let mut record = vec![0.0; vertex.properties.len()];
    // the vertex records in place, for the intensity range of the first points
    let start = values.at;
    let mut range = None;
    if let Some(k) = intensity.filter(|_| rgb.is_none()) {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for _ in 0..total.min(10_000) {
            values.record(vertex, &mut record)?;
            if record[k].is_finite() {
                (lo, hi) = (lo.min(record[k]), hi.max(record[k]));
            }
        }
        range = Some(if lo.is_finite() { intensity_range(lo, hi) } else { (0.0, 1.0) });
        values.at = start;
    }
    for _ in 0..total {
        values.record(vertex, &mut record)?;
        if !cloud.keep() {
            continue;
        }
        let color = match (rgb, intensity.zip(range)) {
            (Some(c), _) => c.map(|k| (record[k] * rgb_scale).round().clamp(0.0, 255.0) as u8),
            (None, Some((k, range))) => grey(record[k], range),
            _ => [0; 3],
        };
        let layer = match class.map(|k| record[k]).filter(|c| c.is_finite()) {
            Some(c) => {
                let c = c.clamp(0.0, 255.0) as u8;
                cloud.layer(c as u32, || Layer { name: class_name(c), color: class_color(c), ..Layer::default() })
            }
            None => cloud.layer(u32::MAX, || Layer { name: stem.to_string(), color: [0.80, 0.80, 0.82], ..Layer::default() }),
        };
        cloud.push(layer, [record[x], record[y], record[z]], color);
    }
    if cloud.kept() == 0 {
        return Err("no points".into());
    }
    let info = cloud.info("PLY", total);
    let mut layers = cloud.into_layers(true);
    let colored = rgb.is_some() || range.is_some();
    if !colored {
        color_by_height(&mut layers);
    }
    let mut info = info;
    info.push(("PLY vertex properties".into(), vertex.properties.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(" ")));
    if !colored {
        info.push(("Point colours".into(), "By height (the file has no colours)".into()));
    }
    Ok((layers, info))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::pointcloud::origin_of;
    use crate::loaders::load_with;
    use std::sync::Arc;

    fn load(name: &str, data: Vec<u8>) -> crate::model::Model {
        load_with(&Source::Bytes { name: name.into(), data: Arc::new(data) }, &Progress::default()).expect(name)
    }

    #[test]
    fn vertices_without_faces_are_a_point_cloud() {
        // binary, double coordinates in RD New (millimetres survive), 8-bit colours, a class
        let mut data = b"ply\nformat binary_little_endian 1.0\ncomment scan\nelement vertex 3\nproperty double x\nproperty double y\nproperty double z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nproperty uchar classification\nelement face 0\nproperty list uchar int vertex_indices\nend_header\n".to_vec();
        for (p, c, class) in [([155_000.001, 463_000.0, 1.0], [255u8, 0, 0], 2u8), ([155_001.0, 463_000.0, 1.0], [0, 255, 0], 2), ([155_000.5, 463_000.5, 6.0], [0, 0, 255], 6)] {
            for v in p {
                data.extend_from_slice(&f64::to_le_bytes(v));
            }
            data.extend_from_slice(&c);
            data.push(class);
        }
        let model = load("scan.ply", data);
        assert_eq!(model.format, "PLY");
        assert_eq!((model.point_count(), model.triangle_count()), (3, 0));
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Ground", "Building"]);
        assert_eq!(origin_of(&model.project.info), Some("155000, 463000, 1"));
        let ground = &model.layers[0];
        assert!((ground.positions[0] - 0.001).abs() < 1e-6);
        assert_eq!(ground.colors.as_ref().unwrap()[1], [0, 255, 0, 255]);
        assert_eq!(model.layers[1].colors.as_ref().unwrap()[0], [0, 0, 255, 255]);
    }

    #[test]
    fn ascii_vertices_with_intensity_and_float_colours() {
        let text = "ply\nformat ascii 1.0\nelement vertex 2\nproperty float x\nproperty float y\nproperty float z\nproperty float scalar_Intensity\nend_header\n0 0 0 0\n1 2 3 255\n";
        let model = load("pts.PLY", text.as_bytes().to_vec());
        let layer = &model.layers[0];
        assert_eq!(layer.name, "pts");
        assert!(layer.points);
        assert_eq!(layer.colors.as_ref().unwrap(), &[[0, 0, 0, 255], [255, 255, 255, 255]]);
        let text = "ply\nformat binary_big_endian 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float red\nproperty float green\nproperty float blue\nend_header\n";
        let mut data = text.as_bytes().to_vec();
        for v in [1.0f32, 2.0, 3.0, 1.0, 0.5, 0.0] {
            data.extend_from_slice(&v.to_be_bytes());
        }
        let model = load("c.ply", data);
        assert_eq!(model.layers[0].positions, [1.0, 2.0, 3.0]);
        assert_eq!(model.layers[0].colors.as_ref().unwrap()[0], [255, 128, 0, 255]);
    }
}
