//! Vertex BD projects (`.vxz` archives, `.vxm` models) and Hundegger BTL machine files (`.btl`,
//! timber members as exported by Vertex BD) through the OpenAEC shared library `vertex.dll`
//! (`libvertex.so` on Linux, `libvertex.dylib` on macOS). The reader is not compiled in: the
//! installers put the library next to the program and it is loaded at run time, so without it these
//! files report that the reader is missing. The interface (`vertex_read`, one scene buffer) is
//! described in the library's `docs/ABI.md`.
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Draft, DraftKind, DraftLine, DraftText, Layer, ProjectData};
use std::path::PathBuf;
use std::sync::OnceLock;

/// The height of a capital letter to that of a line of letters.
const CAPITALS: f32 = 0.72;

/// The interface version this adapter understands.
const ABI_VERSION: u32 = 1;

#[cfg(windows)]
const LIBRARY: &str = "vertex.dll";
#[cfg(target_os = "macos")]
const LIBRARY: &str = "libvertex.dylib";
#[cfg(all(unix, not(target_os = "macos")))]
const LIBRARY: &str = "libvertex.so";

pub struct VertexLoader;

impl Loader for VertexLoader {
    fn name(&self) -> &'static str { "Vertex BD" }
    fn extensions(&self) -> &'static [&'static str] { &["vxz", "vxm"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        read(source, progress)
    }
}

pub struct BtlLoader;

impl Loader for BtlLoader {
    fn name(&self) -> &'static str { "BTL" }
    fn extensions(&self) -> &'static [&'static str] { &["btl"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        read(source, progress)
    }
}

fn read(source: &Source, progress: &Progress) -> Result<Loaded, String> {
    progress.set(Phase::Read);
    let library = library()?;
    let data = source.read()?;
    progress.set(Phase::Convert);
    let scene = library.read(&source.name(), &data)?;
    decode(&scene)
}

/// The functions of the loaded library.
struct Library {
    read: unsafe extern "C" fn(*const u8, usize, *const u8, usize, *mut *mut u8, *mut usize) -> i32,
    free: unsafe extern "C" fn(*mut u8, usize),
    // Keeps the library loaded as long as the functions above are used.
    _lib: libloading::Library,
}

impl Library {
    /// Calls `vertex_read` and copies the buffer it returns (a scene, or an error message).
    fn read(&self, name: &str, data: &[u8]) -> Result<Vec<u8>, String> {
        let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
        // SAFETY: the pointers and lengths describe live slices and out-parameters, and the
        // buffer is copied before it is handed back to `vertex_free`, once.
        let code = unsafe { (self.read)(name.as_ptr(), name.len(), data.as_ptr(), data.len(), &mut out, &mut len) };
        if out.is_null() {
            return Err(format!("{name}: the Vertex reader returned nothing (code {code})"));
        }
        let bytes = unsafe { std::slice::from_raw_parts(out, len) }.to_vec();
        unsafe { (self.free)(out, len) };
        match code {
            0 => Ok(bytes),
            _ => Err(String::from_utf8_lossy(&bytes).into_owned()),
        }
    }
}

/// The library, loaded once: from `O3V_VERTEX_LIB`, next to the program, in `../lib/open-3d-viewer`
/// (the Linux packages) or `../Frameworks` (the macOS app), and in a build from source in
/// `vendor/vertex`.
fn library() -> Result<&'static Library, String> {
    static LIBRARY_CELL: OnceLock<Result<Library, String>> = OnceLock::new();
    LIBRARY_CELL.get_or_init(load_library).as_ref().map_err(Clone::clone)
}

fn candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("O3V_VERTEX_LIB") {
        paths.push(PathBuf::from(path));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(PathBuf::from)) {
        paths.push(dir.join(LIBRARY));
        paths.push(dir.join("../lib/open-3d-viewer").join(LIBRARY));
        paths.push(dir.join("../Frameworks").join(LIBRARY));
    }
    paths.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/vertex").join(LIBRARY));
    paths
}

fn load_library() -> Result<Library, String> {
    let mut tried = Vec::new();
    for path in candidates() {
        if !path.is_file() {
            continue;
        }
        // SAFETY: loading runs the library's initialisers; vertex.dll is our own Rust library
        // without any.
        let lib = match unsafe { libloading::Library::new(&path) } {
            Ok(lib) => lib,
            Err(e) => {
                tried.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        // SAFETY: the symbols have the signatures of the library's include/vertex.h.
        unsafe {
            let version = lib.get::<unsafe extern "C" fn() -> u32>(b"vertex_abi_version\0").map_err(|e| format!("{}: {e}", path.display()))?;
            let version = version();
            if version != ABI_VERSION {
                tried.push(format!("{}: interface version {version}, this viewer needs {ABI_VERSION}", path.display()));
                continue;
            }
            let read = *lib.get(b"vertex_read\0").map_err(|e| format!("{}: {e}", path.display()))?;
            let free = *lib.get(b"vertex_free\0").map_err(|e| format!("{}: {e}", path.display()))?;
            return Ok(Library { read, free, _lib: lib });
        }
    }
    let detail = if tried.is_empty() { String::new() } else { format!(" ({})", tried.join("; ")) };
    Err(format!("Vertex BD and BTL files need the Vertex reader ({LIBRARY}), which comes with the installers of Open BIM Suite{detail}"))
}

/// The scene buffer of the library (`VXS1`, see its docs/ABI.md) as the viewer's layers.
fn decode(buf: &[u8]) -> Result<Loaded, String> {
    let bad = || "Vertex reader: damaged scene".to_string();
    if buf.get(..4) != Some(b"VXS1") {
        return Err(bad());
    }
    let header_len = u32::from_le_bytes(buf.get(4..8).ok_or_else(bad)?.try_into().map_err(|_| bad())?) as usize;
    let header: serde_json::Value = serde_json::from_slice(buf.get(8..8 + header_len).ok_or_else(bad)?).map_err(|_| bad())?;
    let mut at = 8 + header_len;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let bytes = buf.get(at..at.checked_add(n).ok_or_else(bad)?).ok_or_else(bad)?;
        at += n;
        Ok(bytes)
    };
    let floats = |b: &[u8]| b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect::<Vec<_>>();

    let mut project = ProjectData::default();
    for pair in header["info"].as_array().into_iter().flatten() {
        if let (Some(k), Some(v)) = (pair[0].as_str(), pair[1].as_str()) {
            project.info.push((k.to_string(), v.to_string()));
        }
    }
    let mut layers = Vec::new();
    for l in header["layers"].as_array().ok_or_else(bad)? {
        let vertices = l["vertices"].as_u64().ok_or_else(bad)? as usize;
        let indices = l["indices"].as_u64().ok_or_else(bad)? as usize;
        let color = l["color"].as_array().map(|c| [0, 1, 2].map(|i| c.get(i).and_then(|v| v.as_f64()).unwrap_or(0.7) as f32)).unwrap_or(super::DEFAULT_COLOR);
        let positions = floats(take(vertices * 12)?);
        let normals = floats(take(vertices * 12)?);
        let colors = match l["colors"].as_bool() {
            Some(true) => Some(take(vertices * 4)?.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect()),
            _ => None,
        };
        let indices: Vec<u32> = take(indices * 4)?.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        if indices.iter().any(|&i| i as usize >= vertices) {
            return Err(bad());
        }
        let elements = l["elements"].as_array().into_iter().flatten().filter_map(|e| Some((e[0].as_u64()?, e[1].as_u64()? as u32, e[2].as_u64()? as u32))).collect();
        let props = l["props"].as_array().into_iter().flatten().filter_map(|p| Some((p[0].as_u64()?, p[1].as_str()?.to_string()))).collect();
        layers.push(Layer { name: l["name"].as_str().unwrap_or("Layer").to_string(), color, positions, normals, colors, indices, elements, props, hidden: false, points: false, lines: Vec::new(), line_elements: Vec::new(), splats: None });
    }
    // the drawings of a project, after the layers: per drawing its lines (x and y of both ends,
    // metres), the widths of their pens and, when the header says so, their colours; its texts
    // are in the header
    for (k, d) in header["drawings"].as_array().into_iter().flatten().enumerate() {
        let count = d["lines"].as_u64().ok_or_else(bad)? as usize;
        let ends = floats(take(count.checked_mul(16).ok_or_else(bad)?)?);
        let _weights = take(count * 4)?;
        let colours: Vec<[u8; 3]> = match d["colors"].as_bool() {
            Some(true) => take(count * 3)?.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
            _ => Vec::new(),
        };
        // what each line belongs to: lines and texts of one number are picked together
        let parts: Vec<u64> = match d["ids"].as_bool() {
            Some(true) => take(count * 4)?.chunks_exact(4).map(|c| u64::from(u32::from_le_bytes([c[0], c[1], c[2], c[3]]))).collect(),
            _ => Vec::new(),
        };
        let id = k as u64 + 1;
        // a part is known by its drawing and its number in it, so that the parts of two drawings
        // are told apart (a reader that numbers nothing gives the drawing as one thing)
        let within = |part: u64| (id << 32) | part;
        let part = |i: usize| parts.get(i).copied().filter(|p| *p != 0).map_or(id, within);
        for known in d["props"].as_array().into_iter().flatten() {
            if let (Some(number), Some(pairs)) = (known[0].as_u64().filter(|n| *n != 0), known[1].as_str()) {
                project.drawn_pairs.insert(within(number), pairs.to_string());
            }
        }
        let kind = d["kind"].as_str().unwrap_or_default();
        let name = d["name"].as_str().unwrap_or("Drawing");
        let finite = |v: &[f32]| v.iter().all(|x| x.is_finite());
        // a line that is no number is left out, with its colour
        let kept: Vec<usize> = (0..count).filter(|&i| finite(&ends[i * 4..i * 4 + 4])).collect();
        let lines: Vec<DraftLine> = kept.iter().map(|&i| DraftLine { element: part(i), a: [ends[i * 4], ends[i * 4 + 1]], b: [ends[i * 4 + 2], ends[i * 4 + 3]] }).collect();
        // black is the ink of the drawing
        let line_colors: Vec<Option<[u8; 3]>> = if colours.iter().any(|c| *c != [0, 0, 0]) { kept.iter().map(|&i| colours.get(i).copied().filter(|c| *c != [0, 0, 0])).collect() } else { Vec::new() };
        let texts = d["texts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| {
                let number = |i: usize| t.get(i).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).map(|v| v as f32);
                // the height of its capital letters; a line of letters is higher
                let capitals = number(2)?.max(0.0);
                let size = capitals / CAPITALS;
                let mut face = crate::typeface::Face::default();
                if let Some(family) = t.get(6).and_then(|v| v.as_str()).filter(|f| !f.is_empty()) {
                    face.family = family.into();
                }
                let rgb = t.get(7).and_then(|v| v.as_array()).filter(|c| c.len() == 3).map(|c| [0, 1, 2].map(|i| c[i].as_u64().unwrap_or(0).min(255) as u8));
                face.color = rgb.filter(|c| *c != [0, 0, 0]);
                if let Some(width) = number(8).filter(|w| (0.2..=3.0).contains(w)) {
                    face.width = width;
                }
                Some(DraftText {
                    element: t.get(9).and_then(|v| v.as_u64()).filter(|p| *p != 0).map_or(id, within),
                    at: [number(0)?, number(1)?],
                    // its box from the foot of its letters up to the top of a line of them
                    width: 0.0,
                    height: size,
                    size,
                    align: t.get(4).and_then(|v| v.as_u64()).unwrap_or(0).min(2) as u8,
                    angle: number(3)?.to_radians(),
                    text: t.get(5)?.as_str()?.to_string(),
                    face,
                })
            })
            .collect();
        // a sheet, a panel and a piece drawing are on paper already; the others are the model at
        // full size
        let scale = if matches!(kind, "Sheet" | "Panel" | "Piece") { 1.0 } else { 50.0 };
        let name = if kind.is_empty() { name.to_string() } else { format!("{name} ({kind})") };
        project.drafts.push(Draft { id, name, kind: DraftKind::Drawing, scale, lines, texts, dimensions: Vec::new(), line_colors, fills: Vec::new() });
    }
    Ok(Loaded { structural: Vec::new(), origin: None, layers, views: Vec::new(), project })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A scene buffer as the library writes it: one triangle with one element.
    fn scene() -> Vec<u8> {
        let header = r#"{"format":"BTL","info":[["BTL version","BTL V10.5"]],"layers":[{"name":"STUD","color":[0.8,0.6,0.4],"vertices":3,"indices":3,"colors":true,"elements":[[1,0,3]],"props":[[1,"{\"Type\":\"STUD\"}"]]}]}"#;
        let mut buf = b"VXS1".to_vec();
        buf.extend((header.len() as u32).to_le_bytes());
        buf.extend(header.as_bytes());
        for v in [0f32, 0., 0., 1., 0., 0., 0., 1., 0.] { buf.extend(v.to_le_bytes()); }
        for v in [0f32, 0., 1., 0., 0., 1., 0., 0., 1.] { buf.extend(v.to_le_bytes()); }
        buf.extend([200u8, 150, 100, 255].repeat(3));
        for i in [0u32, 1, 2] { buf.extend(i.to_le_bytes()); }
        buf
    }

    #[test]
    fn decodes_a_scene_buffer() {
        let loaded = decode(&scene()).unwrap();
        assert_eq!(loaded.layers.len(), 1);
        let layer = &loaded.layers[0];
        assert_eq!((layer.name.as_str(), layer.vertex_count(), layer.triangle_count()), ("STUD", 3, 1));
        assert_eq!(layer.colors.as_ref().unwrap()[2], [200, 150, 100, 255]);
        assert_eq!(layer.elements, [(1, 0, 3)]);
        assert_eq!(loaded.project.info[0].1, "BTL V10.5");
    }

    /// A scene with one drawing: a line and a text.
    #[test]
    fn decodes_the_drawings_of_a_project() {
        let header = r#"{"format":"Vertex BD","info":[],"layers":[],"drawings":[{"name":"SECT-A","kind":"View","lines":1,"colors":true,"ids":true,"texts":[[1.5,-0.5,0.072,90.0,1,"6900","Courier New",[0,36,170],0.7,1000007]],"props":[[1000007,"[[\"Kind\",\"Dimension\"],[\"Measured (mm)\",\"6900\"]]"]]}]}"#;
        let mut buf = b"VXS1".to_vec();
        buf.extend((header.len() as u32).to_le_bytes());
        buf.extend(header.as_bytes());
        for v in [0f32, 0., 3., 0., 0.18] { buf.extend(v.to_le_bytes()); }
        buf.extend([182u8, 145, 0]);
        buf.extend(1_000_007u32.to_le_bytes());
        let loaded = decode(&buf).unwrap();
        let draft = &loaded.project.drafts[0];
        assert_eq!((draft.name.as_str(), draft.kind, draft.scale), ("SECT-A (View)", DraftKind::Drawing, 50.0));
        // the line and the text of one dimension are one thing to pick
        let part = (1u64 << 32) | 1_000_007;
        assert_eq!(draft.lines, [DraftLine { element: part, a: [0.0, 0.0], b: [3.0, 0.0] }]);
        assert_eq!(draft.texts[0].element, part);
        // what the reader knows of it, kept as it wrote it
        assert_eq!(loaded.project.drawn_pairs[&part], r#"[["Kind","Dimension"],["Measured (mm)","6900"]]"#);
        let text = &draft.texts[0];
        assert_eq!((text.text.as_str(), text.at, text.align), ("6900", [1.5, -0.5], 1));
        assert!((text.size - 0.1).abs() < 1e-6 && (text.height - 0.1).abs() < 1e-6);
        assert_eq!((&*text.face.family, text.face.color, text.face.width), ("Courier New", Some([0, 36, 170]), 0.7));
        assert_eq!(draft.line_colors, [Some([182, 145, 0])]);
        assert!((text.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        // lines the header promises but the buffer lacks
        assert!(decode(&buf[..buf.len() - 2]).is_err());
    }

    #[test]
    fn refuses_damaged_buffers() {
        let full = scene();
        assert!(decode(&full[..full.len() - 1]).is_err());
        assert!(decode(b"VXS2").is_err());
        let mut bad_index = full.clone();
        let n = bad_index.len();
        bad_index[n - 4..].copy_from_slice(&7u32.to_le_bytes());
        assert!(decode(&bad_index).is_err());
    }

    /// With the library in vendor/vertex (or `O3V_VERTEX_LIB`): a real BTL file end to end.
    #[test]
    fn reads_btl_through_the_library_when_present() {
        if library().is_err() {
            return;
        }
        let btl = "VERSION: \"BTL V10.5\"\r\n[GENERAL]\r\nSCALEUNIT: 2\r\n[PART]\r\nANNOTATION: \"STUD\"\r\nLENGTH: 00250000\r\nHEIGHT: 00007000\r\nWIDTH: 00004300\r\nTRANSFORMATION: OX:00000000 OY:00000000 OZ:00000000 XX:00000000 XY:00000000 XZ:00000100 YX:00000100 YY:00000000 YZ:00000000 ZX:00000000 ZY:00000100 ZZ:00000000\r\n";
        let source = Source::Bytes { name: "wall.btl".into(), data: Arc::new(btl.as_bytes().to_vec()) };
        let model = super::super::load_with(&source, &Progress::default()).expect("BTL");
        assert_eq!(model.format, "BTL");
        assert_eq!(model.triangle_count(), 12);
        assert!((model.bounds.max[2] - 2.5).abs() < 1e-4, "{:?}", model.bounds);
    }
}
