//! Stanford PLY (ASCII, binary little/big endian): vertices with optional normals and colours,
//! polygon faces (fan triangulated). Kept as stored (Z up is the common convention for scans).

use super::{Loader, DEFAULT_COLOR};
use crate::model::{smooth_normals, Layer};
use std::path::Path;

pub struct PlyLoader;

impl Loader for PlyLoader {
    fn name(&self) -> &'static str {
        "PLY"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ply"]
    }

    fn load(&self, path: &Path) -> Result<Vec<Layer>, String> {
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        parse(&data, name)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Encoding {
    Ascii,
    Little,
    Big,
}

#[derive(Clone, Copy)]
enum Scalar {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Scalar {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "char" | "int8" => Self::I8,
            "uchar" | "uint8" => Self::U8,
            "short" | "int16" => Self::I16,
            "ushort" | "uint16" => Self::U16,
            "int" | "int32" => Self::I32,
            "uint" | "uint32" => Self::U32,
            "float" | "float32" => Self::F32,
            "double" | "float64" => Self::F64,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 | Self::F32 => 4,
            Self::F64 => 8,
        }
    }
}

struct Property {
    name: String,
    /// `Some(count type)` for list properties.
    list: Option<Scalar>,
    scalar: Scalar,
}

struct Element {
    name: String,
    count: usize,
    properties: Vec<Property>,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    encoding: Encoding,
}

impl Reader<'_> {
    fn token(&mut self) -> Result<&str, String> {
        while self.pos < self.data.len() && self.data[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        let start = self.pos;
        while self.pos < self.data.len() && !self.data[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        if start == self.pos {
            return Err("PLY: unexpected end of file".into());
        }
        std::str::from_utf8(&self.data[start..self.pos]).map_err(|_| "PLY: bad text".to_string())
    }

    fn read(&mut self, t: Scalar) -> Result<f64, String> {
        if self.encoding == Encoding::Ascii {
            return self.token()?.parse::<f64>().map_err(|e| format!("PLY: {e}"));
        }
        let n = t.size();
        let bytes = self.data.get(self.pos..self.pos + n).ok_or("PLY: unexpected end of file")?;
        self.pos += n;
        let mut b = [0u8; 8];
        b[..n].copy_from_slice(bytes);
        if self.encoding == Encoding::Big {
            b[..n].reverse();
        }
        Ok(match t {
            Scalar::I8 => b[0] as i8 as f64,
            Scalar::U8 => b[0] as f64,
            Scalar::I16 => i16::from_le_bytes([b[0], b[1]]) as f64,
            Scalar::U16 => u16::from_le_bytes([b[0], b[1]]) as f64,
            Scalar::I32 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
            Scalar::U32 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
            Scalar::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
            Scalar::F64 => f64::from_le_bytes(b),
        })
    }
}

fn parse(data: &[u8], name: String) -> Result<Vec<Layer>, String> {
    let header_end = data
        .windows(10)
        .position(|w| w == b"end_header")
        .ok_or("PLY: no end_header")?;
    let header = String::from_utf8_lossy(&data[..header_end]);
    let mut body = header_end + 10;
    // skip to the end of the end_header line
    while body < data.len() && data[body] != b'\n' {
        body += 1;
    }
    body += 1;

    let mut encoding = None;
    let mut elements: Vec<Element> = Vec::new();
    for line in header.lines() {
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
            ["element", n, count] => elements.push(Element {
                name: n.to_string(),
                count: count.parse().map_err(|_| "PLY: bad element count")?,
                properties: Vec::new(),
            }),
            ["property", "list", c, t, n] => {
                let e = elements.last_mut().ok_or("PLY: property before element")?;
                e.properties.push(Property {
                    name: n.to_string(),
                    list: Some(Scalar::parse(c).ok_or("PLY: bad list type")?),
                    scalar: Scalar::parse(t).ok_or("PLY: bad type")?,
                });
            }
            ["property", t, n] => {
                let e = elements.last_mut().ok_or("PLY: property before element")?;
                e.properties.push(Property {
                    name: n.to_string(),
                    list: None,
                    scalar: Scalar::parse(t).ok_or("PLY: bad type")?,
                });
            }
            _ => {}
        }
    }
    let encoding = encoding.ok_or("PLY: missing format line")?;
    let mut r = Reader { data, pos: body.min(data.len()), encoding };

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors: Vec<[u8; 4]> = Vec::new();
    let mut indices = Vec::new();
    let mut has_normals = false;
    let mut has_colors = false;

    for e in &elements {
        let find = |n: &str| e.properties.iter().position(|p| p.name == n);
        let (ix, iy, iz) = (find("x"), find("y"), find("z"));
        let (inx, iny, inz) = (find("nx"), find("ny"), find("nz"));
        let (ir, ig, ib) = (
            find("red").or(find("r")),
            find("green").or(find("g")),
            find("blue").or(find("b")),
        );
        let is_vertex = e.name == "vertex";
        let is_face = e.name == "face";
        if is_vertex {
            has_normals = inx.is_some() && iny.is_some() && inz.is_some();
            has_colors = ir.is_some() && ig.is_some() && ib.is_some();
        }
        let color_scale = |p: Option<usize>| match p.map(|i| e.properties[i].scalar) {
            Some(Scalar::F32) | Some(Scalar::F64) => 255.0,
            _ => 1.0,
        };
        let cs = color_scale(ir);
        let mut values = vec![0.0f64; e.properties.len()];
        let mut list: Vec<u32> = Vec::new();
        for _ in 0..e.count {
            list.clear();
            for (pi, p) in e.properties.iter().enumerate() {
                match p.list {
                    Some(ct) => {
                        let n = r.read(ct)? as usize;
                        for _ in 0..n {
                            let v = r.read(p.scalar)?;
                            if is_face && (p.name == "vertex_indices" || p.name == "vertex_index") {
                                list.push(v as u32);
                            }
                        }
                    }
                    None => values[pi] = r.read(p.scalar)?,
                }
            }
            if is_vertex {
                let g = |i: Option<usize>| i.map(|i| values[i] as f32).unwrap_or(0.0);
                positions.extend_from_slice(&[g(ix), g(iy), g(iz)]);
                if has_normals {
                    normals.extend_from_slice(&[g(inx), g(iny), g(inz)]);
                }
                if has_colors {
                    let c = |i: Option<usize>| (g(i) as f64 * cs).clamp(0.0, 255.0) as u8;
                    colors.push([c(ir), c(ig), c(ib), 255]);
                }
            } else if is_face {
                for k in 1..list.len().saturating_sub(1) {
                    indices.extend_from_slice(&[list[0], list[k], list[k + 1]]);
                }
            }
        }
    }

    if indices.is_empty() {
        return Err("PLY: no faces (point clouds are not supported yet)".into());
    }
    let n = (positions.len() / 3) as u32;
    if indices.iter().any(|&i| i >= n) {
        return Err("PLY: face index out of range".into());
    }
    let normals = if has_normals { normals } else { smooth_normals(&positions, &indices) };
    Ok(vec![Layer {
        name,
        color: DEFAULT_COLOR,
        positions,
        normals,
        colors: has_colors.then_some(colors),
        indices,
        ..Layer::default()
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_quad() {
        let ply = b"ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n1 1 0\n0 1 0\n4 0 1 2 3\n";
        let layers = parse(ply, "q".into()).unwrap();
        assert_eq!(layers[0].indices, vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(&layers[0].normals[0..3], &[0.0, 0.0, 1.0]);
    }

    #[test]
    fn binary_triangle_with_colors() {
        let mut d = b"ply\nformat binary_little_endian 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 1\nproperty list uchar uint vertex_indices\nend_header\n".to_vec();
        for (p, c) in [([0.0f32, 0.0, 0.0], 255u8), ([1.0, 0.0, 0.0], 0), ([0.0, 1.0, 0.0], 0)] {
            for v in p {
                d.extend_from_slice(&v.to_le_bytes());
            }
            d.extend_from_slice(&[c, 0, 0]);
        }
        d.push(3);
        for i in [0u32, 1, 2] {
            d.extend_from_slice(&i.to_le_bytes());
        }
        let layers = parse(&d, "t".into()).unwrap();
        assert_eq!(layers[0].indices.len(), 3);
        assert_eq!(layers[0].colors.as_ref().unwrap()[0], [255, 0, 0, 255]);
    }
}
