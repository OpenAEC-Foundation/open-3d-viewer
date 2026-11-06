//! Vertex BD projects (`.vxz` archives, `.vxm` models) and Hundegger BTL machine files (`.btl`,
//! timber members as exported by Vertex BD) through the OpenAEC shared library `vertex.dll`
//! (`libvertex.so` on Linux, `libvertex.dylib` on macOS). The reader is not compiled in: the
//! installers put the library next to the program and it is loaded at run time, so without it these
//! files report that the reader is missing. The interface (`vertex_read`, one scene buffer) is
//! described in the library's `docs/ABI.md`.
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};
use std::path::PathBuf;
use std::sync::OnceLock;

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
    Err(format!("Vertex BD and BTL files need the Vertex reader ({LIBRARY}), which comes with the installers of Open 3D Viewer Studio{detail}"))
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
        layers.push(Layer { name: l["name"].as_str().unwrap_or("Layer").to_string(), color, positions, normals, colors, indices, elements, props, hidden: false, points: false });
    }
    Ok(Loaded { layers, views: Vec::new(), project })
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
