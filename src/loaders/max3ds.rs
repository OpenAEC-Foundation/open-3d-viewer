//! 3D Studio (.3ds), the chunked binary format of 3D Studio and older 3ds Max versions, still
//! written by many converters and model libraries. Each named mesh object is a pickable element
//! with its name; one layer per material, in its diffuse colour (faces without a material in
//! `Default`). Vertices are stored in world space, so the object's local matrix (0x4160) is not
//! needed to place them. Flat shaded; kept Z up, in file units (3DS has no fixed unit). Textures, smoothing groups, lights, cameras and the keyframer (animation,
//! instances) are not read.

use super::{Loader, Source, DEFAULT_COLOR};
use crate::model::Layer;
use glam::Vec3;
use std::collections::{BTreeMap, HashMap};

pub struct Max3dsLoader;

impl Loader for Max3dsLoader {
    fn name(&self) -> &'static str {
        "3DS"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["3ds"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let data = source.read()?;
        read(&data)
    }
}

const MAIN: u16 = 0x4D4D;
const EDITOR: u16 = 0x3D3D;
const OBJECT: u16 = 0x4000;
const TRIMESH: u16 = 0x4100;
const VERTICES: u16 = 0x4110;
const FACES: u16 = 0x4120;
const FACE_MATERIAL: u16 = 0x4130;
const MATERIAL: u16 = 0xAFFF;
const MATERIAL_NAME: u16 = 0xA000;
const DIFFUSE: u16 = 0xA020;
const COLOR_F32: u16 = 0x0010;
const COLOR_U8: u16 = 0x0011;
const COLOR_U8_GAMMA: u16 = 0x0012;
const COLOR_F32_GAMMA: u16 = 0x0013;

/// The chunks in `data`: (id, body).
fn chunks(data: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    let mut pos = 0;
    std::iter::from_fn(move || {
        let header = data.get(pos..pos + 6)?;
        let id = u16::from_le_bytes([header[0], header[1]]);
        let length = u32::from_le_bytes([header[2], header[3], header[4], header[5]]) as usize;
        // a chunk that runs past its parent: take what is there
        let end = pos.checked_add(length.max(6))?.min(data.len());
        let body = &data[pos + 6..end];
        pos = end;
        Some((id, body))
    })
}

/// A zero-terminated string and the bytes after it.
fn cstring(data: &[u8]) -> (String, &[u8]) {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    (String::from_utf8_lossy(&data[..end]).into_owned(), data.get(end + 1..).unwrap_or(&[]))
}

fn u16_at(data: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(i..i + 2)?.try_into().ok()?))
}

fn f32_at(data: &[u8], i: usize) -> Option<f32> {
    Some(f32::from_le_bytes(data.get(i..i + 4)?.try_into().ok()?))
}

/// The colour of a colour chunk (bytes or floats, gamma corrected or not).
fn color(data: &[u8]) -> Option<[f32; 3]> {
    let mut found = None;
    for (id, body) in chunks(data) {
        let c = match id {
            COLOR_U8 | COLOR_U8_GAMMA if body.len() >= 3 => [body[0], body[1], body[2]].map(|v| v as f32 / 255.0),
            COLOR_F32 | COLOR_F32_GAMMA => [f32_at(body, 0)?, f32_at(body, 4)?, f32_at(body, 8)?].map(|v| v.clamp(0.0, 1.0)),
            _ => continue,
        };
        // prefer the gamma corrected colour when both are there
        if found.is_none() || matches!(id, COLOR_U8_GAMMA | COLOR_F32_GAMMA) {
            found = Some(c);
        }
    }
    found
}

fn read(data: &[u8]) -> Result<Vec<Layer>, String> {
    let Some((MAIN, main)) = chunks(data).next() else { return Err("3DS: no 3DS main chunk".into()) };
    let editor = chunks(main).find(|(id, _)| *id == EDITOR).map(|(_, b)| b).ok_or("3DS: no 3D editor chunk")?;
    let mut materials: HashMap<String, [f32; 3]> = HashMap::new();
    for (_, material) in chunks(editor).filter(|(id, _)| *id == MATERIAL) {
        let name = chunks(material).find(|(id, _)| *id == MATERIAL_NAME).map(|(_, b)| cstring(b).0).unwrap_or_default();
        let diffuse = chunks(material).find(|(id, _)| *id == DIFFUSE).and_then(|(_, b)| color(b)).unwrap_or(DEFAULT_COLOR);
        materials.insert(name, diffuse);
    }
    let mut layers: BTreeMap<String, Layer> = BTreeMap::new();
    let mut next_id = 1;
    for (_, object) in chunks(editor).filter(|(id, _)| *id == OBJECT) {
        let (name, rest) = cstring(object);
        for (_, mesh) in chunks(rest).filter(|(id, _)| *id == TRIMESH) {
            let id = next_id;
            next_id += 1;
            add_mesh(&mut layers, &materials, mesh, id, &name);
        }
    }
    if layers.is_empty() {
        return Err("3DS: no mesh objects".into());
    }
    Ok(layers.into_values().collect())
}

fn add_mesh(layers: &mut BTreeMap<String, Layer>, materials: &HashMap<String, [f32; 3]>, mesh: &[u8], id: u64, name: &str) {
    let mut points = Vec::new();
    let mut faces: Vec<[usize; 3]> = Vec::new();
    // material name per face
    let mut face_material: Vec<Option<String>> = Vec::new();
    for (chunk, body) in chunks(mesh) {
        match chunk {
            VERTICES => {
                let n = u16_at(body, 0).unwrap_or(0) as usize;
                points = (0..n)
                    .map_while(|i| Some(Vec3::new(f32_at(body, 2 + 12 * i)?, f32_at(body, 6 + 12 * i)?, f32_at(body, 10 + 12 * i)?)))
                    .collect();
            }
            FACES => {
                let n = u16_at(body, 0).unwrap_or(0) as usize;
                faces = (0..n)
                    .map_while(|i| Some([u16_at(body, 2 + 8 * i)? as usize, u16_at(body, 4 + 8 * i)? as usize, u16_at(body, 6 + 8 * i)? as usize]))
                    .collect();
                face_material = vec![None; faces.len()];
                // the material groups follow the face list
                for (sub, group) in chunks(body.get(2 + 8 * n..).unwrap_or(&[])) {
                    if sub != FACE_MATERIAL {
                        continue;
                    }
                    let (material, list) = cstring(group);
                    let count = u16_at(list, 0).unwrap_or(0) as usize;
                    for k in 0..count {
                        if let Some(slot) = u16_at(list, 2 + 2 * k).and_then(|f| face_material.get_mut(f as usize)) {
                            *slot = Some(material.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut props_written = false;
    // group faces by material so each layer gets one contiguous range per object
    let mut groups: BTreeMap<Option<&String>, Vec<[usize; 3]>> = BTreeMap::new();
    for (face, material) in faces.iter().zip(&face_material) {
        groups.entry(material.as_ref()).or_default().push(*face);
    }
    for (material, faces) in groups {
        let key = material.cloned().unwrap_or_else(|| "Default".into());
        let color = material.and_then(|m| materials.get(m)).copied().unwrap_or(DEFAULT_COLOR);
        let layer = layers.entry(key.clone()).or_insert_with(|| Layer { name: key, color, ..Layer::default() });
        let first = layer.indices.len() as u32;
        for f in faces {
            if f.iter().any(|&i| i >= points.len()) {
                continue;
            }
            let p = f.map(|i| points[i]);
            let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            for v in p {
                layer.indices.push(layer.vertex_count() as u32);
                layer.positions.extend_from_slice(&v.to_array());
                layer.normals.extend_from_slice(&n.to_array());
            }
        }
        let count = layer.indices.len() as u32 - first;
        if count > 0 {
            layer.elements.push((id, first, count));
            if !props_written {
                let mut props = serde_json::Map::new();
                props.insert("Name".into(), name.into());
                layer.props.push((id, serde_json::Value::Object(props).to_string()));
                props_written = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn chunk(id: u16, body: &[u8]) -> Vec<u8> {
        let mut out = id.to_le_bytes().to_vec();
        out.extend((body.len() as u32 + 6).to_le_bytes());
        out.extend(body);
        out
    }

    /// A unit square of two triangles: the first in material `Brick` (red), the second without.
    fn file() -> Vec<u8> {
        let material = [chunk(MATERIAL_NAME, b"Brick\0"), chunk(DIFFUSE, &chunk(COLOR_U8, &[255, 0, 0]))].concat();
        let mut vertices = 4u16.to_le_bytes().to_vec();
        for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]] {
            vertices.extend(v.iter().flat_map(|c| c.to_le_bytes()));
        }
        let mut faces = 2u16.to_le_bytes().to_vec();
        for f in [[0u16, 1, 2, 0], [0, 2, 3, 0]] {
            faces.extend(f.iter().flat_map(|i| i.to_le_bytes()));
        }
        faces.extend(chunk(FACE_MATERIAL, &[b"Brick\0".as_slice(), &1u16.to_le_bytes(), &0u16.to_le_bytes()].concat()));
        // the local matrix (identity), which the reader skips
        let matrix: Vec<u8> = [1.0f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let mesh = [chunk(VERTICES, &vertices), chunk(FACES, &faces), chunk(0x4160, &matrix)].concat();
        let object = [b"Wall\0".as_slice(), &chunk(TRIMESH, &mesh)].concat();
        let editor = [chunk(MATERIAL, &material), chunk(OBJECT, &object)].concat();
        chunk(MAIN, &[chunk(0x0002, &3u32.to_le_bytes()), chunk(EDITOR, &editor)].concat())
    }

    #[test]
    fn reads_objects_and_materials() {
        let source = Source::Bytes { name: "wall.3DS".into(), data: Arc::new(file()) };
        let model = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("3DS");
        assert_eq!(model.format, "3DS");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Brick", "Default"]);
        assert_eq!(model.layers[0].color, [1.0, 0.0, 0.0]);
        assert_eq!(model.triangle_count(), 2);
        assert_eq!(model.bounds.max, [1.0, 1.0, 0.0]);
        assert!(model.layers.iter().all(|l| l.normals.chunks_exact(3).all(|n| n == [0.0, 0.0, 1.0])));
        assert!(model.props.values().all(|p| p.contains("Wall")));
        assert_eq!(model.props.len(), 1);
    }

    #[test]
    fn rejects_other_files() {
        assert!(read(b"not a 3ds file").is_err());
    }
}
