//! MagicaVoxel `.vox` files as coloured triangle meshes.
//!
//! The file is read by `dot_vox`. Each model's voxels are meshed once: only the faces between a
//! voxel and empty space are kept, and neighbouring faces of one colour in one plane are merged
//! into a single rectangle (greedy meshing), so a large model stays light. Each shape of the
//! scene graph then places its model by the transforms above it (rotation and translation of
//! the first key frame), as MagicaVoxel shows it: a model's centre is its voxel box's middle,
//! rounded down. A file without a scene graph (older versions) puts each model at the origin.
//!
//! One voxel is one unit; the file holds no unit of its own. Z is up, as in MagicaVoxel.

use dot_vox::{DotVoxData, Model, SceneNode};
use std::collections::HashMap;

/// One placed model: triangles with a colour per vertex.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// The name of the shape's transform in the scene, or `Model <n>`.
    pub name: String,
    /// xyz per vertex, Z up.
    pub positions: Vec<f32>,
    /// xyz per vertex, unit length.
    pub normals: Vec<f32>,
    /// sRGB colour per vertex from the palette (RGBA8).
    pub colors: Vec<[u8; 4]>,
    /// Triangle list.
    pub indices: Vec<u32>,
    /// Hidden in the file (the node or its layer is switched off).
    pub hidden: bool,
    /// What the file says of the model: its number, size and voxel count.
    pub props: Vec<(String, String)>,
}

/// Reads a `.vox` file.
pub fn read(data: &[u8]) -> Result<Vec<Mesh>, String> {
    let vox = dot_vox::load_bytes(data).map_err(|e| format!("MagicaVoxel: {e}"))?;
    Ok(meshes(&vox))
}

/// The placed meshes of a parsed file.
pub fn meshes(vox: &DotVoxData) -> Vec<Mesh> {
    let mut cache: HashMap<u32, Faces> = HashMap::new();
    let mut out = Vec::new();
    let mut place = |model_id: u32, name: Option<String>, hidden: bool, pose: Pose, centred: bool| {
        let Some(model) = vox.models.get(model_id as usize) else { return };
        let faces = cache.entry(model_id).or_insert_with(|| greedy(model));
        if faces.quads.is_empty() {
            return;
        }
        let half = if centred { [(model.size.x / 2) as f32, (model.size.y / 2) as f32, (model.size.z / 2) as f32] } else { [0.0; 3] };
        let mut mesh = faces.place(&vox.palette, &pose, half);
        mesh.name = name.unwrap_or_else(|| format!("Model {model_id}"));
        mesh.hidden = hidden;
        mesh.props = vec![
            ("Model".into(), model_id.to_string()),
            ("Size".into(), format!("{} × {} × {}", model.size.x, model.size.y, model.size.z)),
            ("Voxels".into(), model.voxels.len().to_string()),
        ];
        out.push(mesh);
    };
    if vox.scenes.is_empty() {
        for id in 0..vox.models.len() as u32 {
            place(id, None, false, Pose::IDENTITY, false);
        }
    } else {
        let layer_hidden = |id: u32| vox.layers.get(id as usize).is_some_and(|l| l.hidden());
        // depth-first through the graph from the root, with the pose, the nearest name and
        // whether anything above is hidden; a visited set guards against cycles in a bad file
        let mut stack = vec![(0u32, Pose::IDENTITY, None::<String>, false)];
        let mut seen = vec![false; vox.scenes.len()];
        while let Some((index, pose, name, hidden)) = stack.pop() {
            let Some(node) = vox.scenes.get(index as usize) else { continue };
            if std::mem::replace(&mut seen[index as usize], true) {
                continue;
            }
            match node {
                SceneNode::Transform { attributes, frames, child, layer_id } => {
                    let frame = frames.first();
                    let rotation = frame.and_then(|f| f.orientation()).map(|r| r.to_cols_array_2d()).unwrap_or(IDENTITY);
                    let t = frame.and_then(|f| f.position()).map(|p| [p.x as f32, p.y as f32, p.z as f32]).unwrap_or([0.0; 3]);
                    let pose = pose.then(&Pose { rotation, translation: t });
                    let name = attributes.get("_name").cloned().or(name);
                    let hidden = hidden || attributes.get("_hidden").is_some_and(|h| h == "1") || layer_hidden(*layer_id);
                    stack.push((*child, pose, name, hidden));
                }
                SceneNode::Group { children, attributes } => {
                    let hidden = hidden || attributes.get("_hidden").is_some_and(|h| h == "1");
                    for child in children.iter().rev() {
                        stack.push((*child, pose.clone(), name.clone(), hidden));
                    }
                }
                SceneNode::Shape { models, .. } => {
                    for shape in models {
                        place(shape.model_id, name.clone(), hidden, pose.clone(), true);
                    }
                }
            }
        }
    }
    out
}

const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// A rotation (columns) and a translation.
#[derive(Debug, Clone, PartialEq)]
struct Pose {
    rotation: [[f32; 3]; 3],
    translation: [f32; 3],
}

impl Pose {
    const IDENTITY: Pose = Pose { rotation: IDENTITY, translation: [0.0; 3] };

    fn rotate(&self, v: [f32; 3]) -> [f32; 3] {
        let c = &self.rotation;
        [0, 1, 2].map(|r| c[0][r] * v[0] + c[1][r] * v[1] + c[2][r] * v[2])
    }

    fn apply(&self, v: [f32; 3]) -> [f32; 3] {
        let r = self.rotate(v);
        [r[0] + self.translation[0], r[1] + self.translation[1], r[2] + self.translation[2]]
    }

    /// This pose followed inside by `inner`: first `inner`, then `self`.
    fn then(&self, inner: &Pose) -> Pose {
        let rotation = inner.rotation.map(|col| self.rotate(col));
        Pose { rotation, translation: self.apply(inner.translation) }
    }

    fn mirrors(&self) -> bool {
        let [a, b, c] = self.rotation;
        let cross = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        cross[0] * c[0] + cross[1] * c[1] + cross[2] * c[2] < 0.0
    }
}

/// A merged face: its four corners in voxel coordinates (counter-clockwise seen from outside),
/// its outward axis and side, and its palette index.
#[derive(Debug, Clone, PartialEq)]
struct Quad {
    corners: [[f32; 3]; 4],
    normal: [f32; 3],
    color: u8,
}

/// The merged faces of one model.
struct Faces {
    quads: Vec<Quad>,
}

impl Faces {
    fn place(&self, palette: &[dot_vox::Color], pose: &Pose, half: [f32; 3]) -> Mesh {
        let flip = pose.mirrors();
        let mut mesh = Mesh::default();
        for quad in &self.quads {
            let base = (mesh.positions.len() / 3) as u32;
            let n = pose.rotate(quad.normal);
            let c = palette.get(quad.color as usize).map(|c| [c.r, c.g, c.b, c.a]).unwrap_or([200, 200, 200, 255]);
            for p in quad.corners {
                let p = pose.apply([p[0] - half[0], p[1] - half[1], p[2] - half[2]]);
                mesh.positions.extend_from_slice(&p);
                mesh.normals.extend_from_slice(&n);
                mesh.colors.push(c);
            }
            let tris: [u32; 6] = if flip { [0, 2, 1, 0, 3, 2] } else { [0, 1, 2, 0, 2, 3] };
            mesh.indices.extend(tris.iter().map(|k| base + k));
        }
        mesh
    }
}

/// The visible faces of a model, merged per plane and colour.
fn greedy(model: &Model) -> Faces {
    let dims = [model.size.x as usize, model.size.y as usize, model.size.z as usize];
    let mut grid = vec![0u16; dims[0] * dims[1] * dims[2]];
    let at = |p: [usize; 3]| p[0] + dims[0] * (p[1] + dims[1] * p[2]);
    for v in &model.voxels {
        let p = [v.x as usize, v.y as usize, v.z as usize];
        if p[0] < dims[0] && p[1] < dims[1] && p[2] < dims[2] {
            grid[at(p)] = v.i as u16 + 1;
        }
    }
    let filled = |p: [isize; 3]| -> u16 {
        if (0..3).any(|k| p[k] < 0 || p[k] as usize >= dims[k]) {
            return 0;
        }
        grid[at([p[0] as usize, p[1] as usize, p[2] as usize])]
    };
    let mut quads = Vec::new();
    for d in 0..3 {
        let (u, v) = ((d + 1) % 3, (d + 2) % 3);
        let mut mask = vec![0u16; dims[u] * dims[v]];
        for positive in [false, true] {
            let step: isize = if positive { 1 } else { -1 };
            for s in 0..dims[d] {
                for j in 0..dims[v] {
                    for i in 0..dims[u] {
                        let mut p = [0isize; 3];
                        p[d] = s as isize;
                        p[u] = i as isize;
                        p[v] = j as isize;
                        let here = filled(p);
                        p[d] += step;
                        mask[i + j * dims[u]] = if here != 0 && filled(p) == 0 { here } else { 0 };
                    }
                }
                let plane = if positive { s + 1 } else { s } as f32;
                for j in 0..dims[v] {
                    let mut i = 0;
                    while i < dims[u] {
                        let c = mask[i + j * dims[u]];
                        if c == 0 {
                            i += 1;
                            continue;
                        }
                        let mut w = 1;
                        while i + w < dims[u] && mask[i + w + j * dims[u]] == c {
                            w += 1;
                        }
                        let mut h = 1;
                        'rows: while j + h < dims[v] {
                            for k in 0..w {
                                if mask[i + k + (j + h) * dims[u]] != c {
                                    break 'rows;
                                }
                            }
                            h += 1;
                        }
                        for jj in 0..h {
                            for k in 0..w {
                                mask[i + k + (j + jj) * dims[u]] = 0;
                            }
                        }
                        let corner = |a: usize, b: usize| {
                            let mut q = [0.0f32; 3];
                            q[d] = plane;
                            q[u] = a as f32;
                            q[v] = b as f32;
                            q
                        };
                        let mut corners = [corner(i, j), corner(i + w, j), corner(i + w, j + h), corner(i, j + h)];
                        if !positive {
                            corners.reverse();
                        }
                        let mut normal = [0.0f32; 3];
                        normal[d] = step as f32;
                        quads.push(Quad { corners, normal, color: (c - 1) as u8 });
                        i += w;
                    }
                }
            }
        }
    }
    Faces { quads }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dot_vox::{Color, Dict, Frame, ShapeModel, Size, Voxel};

    fn palette() -> Vec<Color> {
        (0..256).map(|k| Color { r: k as u8, g: 10, b: 20, a: 255 }).collect()
    }

    fn model(size: [u32; 3], voxels: &[[u8; 4]]) -> Model {
        Model { size: Size { x: size[0], y: size[1], z: size[2] }, voxels: voxels.iter().map(|v| Voxel { x: v[0], y: v[1], z: v[2], i: v[3] }).collect() }
    }

    fn data(models: Vec<Model>, scenes: Vec<SceneNode>) -> DotVoxData {
        DotVoxData { version: 150, index_map: Vec::new(), models, palette: palette(), materials: Vec::new(), scenes, layers: Vec::new() }
    }

    fn bounds(mesh: &Mesh) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in mesh.positions.chunks(3) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }

    #[test]
    fn a_solid_block_of_one_colour_is_six_rectangles() {
        let mut voxels = Vec::new();
        for x in 0..4 {
            for y in 0..3 {
                for z in 0..2 {
                    voxels.push([x, y, z, 7]);
                }
            }
        }
        let meshes = meshes(&data(vec![model([4, 3, 2], &voxels)], Vec::new()));
        assert_eq!(meshes.len(), 1);
        let mesh = &meshes[0];
        assert_eq!(mesh.indices.len(), 6 * 6, "one quad per side");
        assert_eq!(bounds(mesh), ([0.0; 3], [4.0, 3.0, 2.0]));
        assert!(mesh.colors.iter().all(|c| *c == [7, 10, 20, 255]));
        assert_eq!(mesh.name, "Model 0");
    }

    #[test]
    fn faces_point_outward_and_split_by_colour() {
        // two voxels side by side in different colours: their shared face is hidden
        let meshes = meshes(&data(vec![model([2, 1, 1], &[[0, 0, 0, 1], [1, 0, 0, 2]])], Vec::new()));
        let mesh = &meshes[0];
        assert_eq!(mesh.indices.len() / 6, 10, "5 faces of each voxel");
        for tri in mesh.indices.chunks(3) {
            let p = |k: u32| &mesh.positions[k as usize * 3..k as usize * 3 + 3];
            let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let cross = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            let n = &mesh.normals[tri[0] as usize * 3..tri[0] as usize * 3 + 3];
            assert!(cross[0] * n[0] + cross[1] * n[1] + cross[2] * n[2] > 0.0, "winding agrees with the normal");
        }
    }

    #[test]
    fn the_scene_places_and_names_each_shape() {
        let frame = |t: &str, r: Option<&str>| {
            let mut d = Dict::new();
            d.insert("_t".into(), t.into());
            if let Some(r) = r {
                d.insert("_r".into(), r.into());
            }
            Frame::new(d)
        };
        let named = |name: &str| {
            let mut d = Dict::new();
            d.insert("_name".into(), name.into());
            d
        };
        let scenes = vec![
            SceneNode::Transform { attributes: Dict::new(), frames: vec![Frame::default()], child: 1, layer_id: u32::MAX },
            SceneNode::Group { attributes: Dict::new(), children: vec![2, 4] },
            SceneNode::Transform { attributes: named("tower"), frames: vec![frame("10 0 0", None)], child: 3, layer_id: 0 },
            SceneNode::Shape { attributes: Dict::new(), models: vec![ShapeModel { model_id: 0, attributes: Dict::new() }] },
            // the same model again, turned a quarter about Z: (x, y) -> (-y, x)
            SceneNode::Transform { attributes: named("turned"), frames: vec![frame("0 0 0", Some("17"))], child: 5, layer_id: 0 },
            SceneNode::Shape { attributes: Dict::new(), models: vec![ShapeModel { model_id: 0, attributes: Dict::new() }] },
        ];
        let tall = model([2, 4, 6], &(0..6).map(|z| [0, 0, z as u8, 3]).collect::<Vec<_>>());
        let meshes = meshes(&data(vec![tall], scenes));
        assert_eq!(meshes.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["tower", "turned"]);
        // centred on (1, 2, 3): the column at voxel x 0, y 0 spans -1..0, -2..-1, -3..3
        assert_eq!(bounds(&meshes[0]), ([9.0, -2.0, -3.0], [10.0, -1.0, 3.0]));
        let (lo, hi) = bounds(&meshes[1]);
        assert_eq!((lo[2], hi[2]), (-3.0, 3.0));
        assert_eq!(hi[0] - lo[0], 1.0);
        assert_eq!(hi[1] - lo[1], 1.0);
        assert_ne!(lo, bounds(&meshes[0]).0, "the turn moves the column");
    }

    #[test]
    fn round_trips_through_the_file_format() {
        let vox = data(vec![model([1, 1, 1], &[[0, 0, 0, 5]])], Vec::new());
        let mut bytes = Vec::new();
        vox.write_vox(&mut bytes).unwrap();
        let meshes = read(&bytes).unwrap();
        assert_eq!(meshes.len(), 1);
        assert_eq!(meshes[0].indices.len(), 36);
        assert!(read(b"not a vox file").is_err());
    }
}
