//! Adapter from the independent vertex-reader (repository `vertex.dll`) to the app's scene model:
//! Vertex BD projects (`.vxz` archives and their `.vxm` models) and Hundegger BTL machine files
//! (`.btl`, timber members as exported by Vertex BD and other timber CAD).
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};
use vertex_reader::{btl, vxm};

/// Millimetres to metres.
const MM: f64 = 0.001;

pub struct VertexLoader;

impl Loader for VertexLoader {
    fn name(&self) -> &'static str { "Vertex BD" }
    fn extensions(&self) -> &'static [&'static str] { &["vxz", "vxm"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        let model = if source.extension() == "vxm" { vxm::read_vxm(&data)? } else { vxm::read_vxz(&data)? };
        progress.set(Phase::Convert);
        let mut project = ProjectData::default();
        for note in &model.notes {
            // "VOF022947/FRAMING.vxm: build: r199074": the file, then a key and its value.
            let note = note.split_once(": ").filter(|(file, _)| file.ends_with(".vxm")).map_or(note.as_str(), |(_, rest)| rest);
            if let Some((key, value)) = note.split_once(": ") {
                if !project.info.iter().any(|(k, v)| k == key && v == value) {
                    project.info.push((key.to_string(), value.to_string()));
                }
            }
        }
        Ok(Loaded { layers: vxm_layers(model.meshes), views: Vec::new(), project })
    }
}

/// One layer per Vertex model file (FRAMING, MODEL), each body a pickable element with its own
/// colour; flat-shaded, as the bodies are planar faces.
fn vxm_layers(meshes: Vec<vxm::VxmMesh>) -> Vec<Layer> {
    let mut layers: Vec<Layer> = Vec::new();
    for (i, mesh) in meshes.into_iter().enumerate() {
        let name = mesh.kind.split('/').next().filter(|s| !s.is_empty()).unwrap_or("Model").to_string();
        let k = match layers.iter().position(|l| l.name == name) {
            Some(k) => k,
            None => {
                let color = timber_color(layers.len());
                layers.push(Layer { name, color, colors: Some(Vec::new()), ..Default::default() });
                layers.len() - 1
            }
        };
        let layer = &mut layers[k];
        let rgba = mesh.color.unwrap_or([layer.color[0], layer.color[1], layer.color[2], 1.0]).map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        let id = i as u64 + 1;
        let first = layer.indices.len() as u32;
        for tri in mesh.indices.chunks_exact(3) {
            let Some(p) = tri.iter().map(|&j| mesh.positions.get(j as usize).copied()).collect::<Option<Vec<_>>>() else { continue };
            let n = face_normal(p[0], p[1], p[2]);
            let base = layer.vertex_count() as u32;
            for q in &p {
                layer.positions.extend(q.map(|c| (c as f64 * MM) as f32));
                layer.normals.extend(n);
            }
            if let Some(colors) = layer.colors.as_mut() {
                colors.extend([rgba; 3]);
            }
            layer.indices.extend([base, base + 1, base + 2]);
        }
        let count = layer.indices.len() as u32 - first;
        if count == 0 {
            continue;
        }
        layer.elements.push((id, first, count));
        let mut props = serde_json::Map::new();
        props.insert("Name".into(), mesh.name.into());
        for (key, value) in mesh.properties {
            props.insert(key, value.into());
        }
        layer.props.push((id, serde_json::Value::Object(props).to_string()));
    }
    layers.retain(|l| !l.indices.is_empty());
    layers
}

fn face_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l > 0.0 { n.map(|x| x / l) } else { [0.0, 0.0, 1.0] }
}

pub struct BtlLoader;

impl Loader for BtlLoader {
    fn name(&self) -> &'static str { "BTL" }
    fn extensions(&self) -> &'static [&'static str] { &["btl"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        if !btl::is_btl(&data) {
            return Err(format!("{}: not a BTL file", source.name()));
        }
        let file = btl::parse(&data)?;
        progress.set(Phase::Convert);
        Ok(Loaded { layers: btl_layers(&file), views: Vec::new(), project: btl_project(&file) })
    }
}

/// The project info: version, exporter and the filled-in `[GENERAL]` keys.
fn btl_project(file: &btl::BtlFile) -> ProjectData {
    let mut project = ProjectData::default();
    project.info.push(("BTL version".into(), file.version.clone()));
    let comments: Vec<&str> = file.general.iter().filter(|(k, _)| k == "COMMENT").map(|(_, v)| v.as_str()).collect();
    for (key, value) in &file.general {
        if key != "COMMENT" && key != "SCALEUNIT" && !value.trim().is_empty() {
            project.info.push((title_case(key), value.trim().to_string()));
        }
    }
    if !comments.is_empty() {
        project.info.push(("Comment".into(), comments.join("; ")));
    }
    project.info.push(("Parts".into(), file.parts.len().to_string()));
    project
}

/// One layer per member type (`ANNOTATION`), each part a pickable element.
fn btl_layers(file: &btl::BtlFile) -> Vec<Layer> {
    let mut layers: Vec<Layer> = Vec::new();
    for (i, part) in file.parts.iter().enumerate() {
        let name = if part.annotation.trim().is_empty() { "Parts".to_string() } else { part.annotation.trim().to_string() };
        let k = match layers.iter().position(|l| l.name == name) {
            Some(k) => k,
            None => {
                let color = timber_color(layers.len());
                layers.push(Layer { name, color, ..Default::default() });
                layers.len() - 1
            }
        };
        let layer = &mut layers[k];
        let mesh = btl::part_mesh(part);
        if mesh.indices.is_empty() {
            continue;
        }
        let id = i as u64 + 1;
        let base = layer.vertex_count() as u32;
        let first = layer.indices.len() as u32;
        layer.positions.extend(mesh.positions.iter().flat_map(|p| p.map(|c| (c as f64 * MM) as f32)));
        layer.normals.extend(mesh.normals.iter().flatten());
        layer.indices.extend(mesh.indices.iter().map(|&j| base + j));
        layer.elements.push((id, first, mesh.indices.len() as u32));
        layer.props.push((id, part_props(part)));
    }
    layers.retain(|l| !l.indices.is_empty());
    layers
}

/// The properties of a part: identification, size and its processings.
fn part_props(part: &btl::BtlPart) -> String {
    let mut map = serde_json::Map::new();
    let mut put = |key: &str, value: serde_json::Value| {
        map.insert(key.to_string(), value);
    };
    if let Some(n) = part.number { put("Member number", n.into()); }
    for (key, label) in [("DESIGNATION", "Designation"), ("ANNOTATION", "Type"), ("STOREY", "Storey"), ("GROUP", "Group"), ("PACKAGE", "Package"), ("ASSEMBLYNUMBER", "Assembly"), ("ORDERNUMBER", "Order"), ("MATERIAL", "Material"), ("TIMBERGRADE", "Timber grade"), ("QUALITYGRADE", "Quality grade"), ("COMMENT", "Comment")] {
        if let Some(v) = part.field(key).filter(|v| !v.trim().is_empty()) {
            put(label, v.trim().into());
        }
    }
    put("Count", part.count.into());
    put("Length (mm)", rounded(part.length));
    put("Height (mm)", rounded(part.height));
    put("Width (mm)", rounded(part.width));
    put("Volume (m³)", serde_json::json!((part.length * part.height * part.width * 1e-9 * 1e6).round() / 1e6));
    if let Some(uid) = part.uid { put("UID", uid.into()); }
    for (n, process) in part.processes.iter().enumerate() {
        let params: Vec<String> = process.params.iter().map(|(k, v)| format!("P{k:02}={v}")).collect();
        put(&format!("Processing {:02}", n + 1), format!("{} ({}) {}", process.key.name(), process.key, params.join(" ")).into());
    }
    serde_json::Value::Object(map).to_string()
}

fn rounded(value: f64) -> serde_json::Value {
    serde_json::json!((value * 100.0).round() / 100.0)
}

/// `PROJECTNUMBER` → "Projectnumber"; the BTL keys have no word breaks.
fn title_case(key: &str) -> String {
    let lower = key.to_lowercase();
    let mut chars = lower.chars();
    chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

/// Wood tones, one per member type.
fn timber_color(i: usize) -> [f32; 3] {
    const TONES: [[f32; 3]; 8] = [
        [0.85, 0.70, 0.48],
        [0.76, 0.58, 0.38],
        [0.90, 0.78, 0.58],
        [0.66, 0.50, 0.34],
        [0.80, 0.66, 0.52],
        [0.72, 0.62, 0.45],
        [0.93, 0.84, 0.66],
        [0.60, 0.46, 0.32],
    ];
    TONES[i % TONES.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const SAMPLE: &str = "VERSION: \"BTL V10.5\"\r\nBUILD: \"10500\"\r\n[GENERAL]\r\nPROJECTNUMBER: \"VOR000001\"\r\nPROJECTNAME: \"Test\"\r\nEXPORTRELEASE: \"Vertex BD Pro 32.0.8 \"\r\nSCALEUNIT: 2\r\nCOMMENT: panel W1\r\n[PART]\r\nSINGLEMEMBERNUMBER: 1\r\nDESIGNATION: \"W1-1\"\r\nANNOTATION: \"STUD\"\r\nTIMBERGRADE: \"C24\"\r\nCOUNT: 1\r\nLENGTH: 00250000\r\nHEIGHT: 00007000\r\nWIDTH: 00004300\r\nUID: 1\r\nTRANSFORMATION: OX:00000000 OY:00000000 OZ:00000000 XX:00000000 XY:00000000 XZ:00000100 YX:00000100 YY:00000000 YZ:00000000 ZX:00000000 ZY:00000100 ZZ:00000000\r\nPROCESSKEY: 2-010-1\r\nPROCESSPARAMETERS:  P01:00000000 P02:00000000 P03:00000000 P06:00009000 P07:00009000\r\nPROCESSIDENT: 1\r\nPROCESSKEY: 1-010-1\r\nPROCESSPARAMETERS:  P01:00250000 P02:00000000 P03:00000000 P06:00009000 P07:00009000\r\nPROCESSIDENT: 2\r\n[PART]\r\nSINGLEMEMBERNUMBER: 2\r\nANNOTATION: \"TOP PLATE\"\r\nLENGTH: 00100000\r\nHEIGHT: 00007000\r\nWIDTH: 00004300\r\nTRANSFORMATION: OX:00000000 OY:00000000 OZ:00250000 XX:00000100 XY:00000000 XZ:00000000 YX:00000000 YY:00000100 YZ:00000000 ZX:00000000 ZY:00000000 ZZ:00000100\r\nPROCESSKEY: 4-040-1\r\nPROCESSPARAMETERS:  P01:00050000 P02:00003500 P11:00004300 P12:00001200\r\nPROCESSIDENT: 1\r\n";

    #[test]
    fn loads_vertex_sample_projects() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../vertex.dll/vertex");
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        let mut loaded = 0;
        for path in entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "vxz")) {
            let model = super::super::load(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(model.format, "Vertex BD");
            assert!(model.layers.iter().all(|l| l.colors.as_ref().is_some_and(|c| c.len() == l.vertex_count())));
            loaded += 1;
        }
        assert!(loaded == 0 || loaded >= 10);
    }

    #[test]
    fn loads_btl_parts_as_elements() {
        let source = Source::Bytes { name: "wall.BTL".into(), data: Arc::new(SAMPLE.as_bytes().to_vec()) };
        let model = super::super::load_with(&source, &Progress::default()).expect("BTL");
        assert_eq!(model.format, "BTL");
        assert_eq!(model.layers.len(), 2);
        assert_eq!(model.triangle_count(), 24);
        // A 2.5 m stud standing up, a 1 m top plate on it: metres, Z up.
        assert!((model.bounds.max[2] - 2.543).abs() < 1e-4, "{:?}", model.bounds);
        assert!(model.props.values().any(|p| p.contains("\"Timber grade\":\"C24\"")));
        assert!(model.props.values().any(|p| p.contains("Drilling (4-040-1)")));
        assert!(model.project.info.iter().any(|(k, v)| k == "Exportrelease" && v.contains("Vertex BD")));
    }
}
