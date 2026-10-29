//! The neutral scene model every loader produces: Z-up, metres (or file units), one
//! triangle mesh per layer.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// One layer: a named triangle mesh with a single base colour (plus optional vertex colours).
#[derive(Debug, Clone, Default)]
pub struct Layer {
    pub name: String,
    /// Base colour (sRGB, 0..1).
    pub color: [f32; 3],
    /// xyz per vertex, Z up.
    pub positions: Vec<f32>,
    /// xyz per vertex, unit length; same length as `positions`.
    pub normals: Vec<f32>,
    /// Optional per-vertex sRGB colours (RGBA8); overrides `color` when present.
    pub colors: Option<Vec<[u8; 4]>>,
    /// Triangle list.
    pub indices: Vec<u32>,
    /// Pickable elements (BIM formats): element id, first index and index count in `indices`,
    /// sorted by first index.
    pub elements: Vec<(u64, u32, u32)>,
    /// Element properties as flat JSON objects; moved into [`Model::props`] by [`Model::new`].
    pub props: Vec<(u64, String)>,
    /// Hidden in the file (a hidden SketchUp tag): starts switched off.
    pub hidden: bool,
}

impl Layer {
    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The triangles of the elements (or the whole layer) that enclose volume, for the cut fill.
    pub fn solid_indices(&self) -> Vec<u32> {
        let whole = [(0u64, 0u32, self.indices.len() as u32)];
        let ranges = if self.elements.is_empty() { &whole[..] } else { &self.elements[..] };
        let mut out = Vec::new();
        for &(_, first, count) in ranges {
            let Some(tris) = self.indices.get(first as usize..(first + count) as usize) else { continue };
            if tris.iter().all(|&i| 3 * (i as usize) + 2 < self.positions.len()) && encloses_volume(&self.positions, tris) {
                out.extend_from_slice(tris);
            }
        }
        out
    }

    /// The element drawn by triangle `tri`, if the layer has elements.
    pub fn element_at(&self, tri: usize) -> Option<u64> {
        let at = (3 * tri) as u32;
        let k = self.elements.partition_point(|e| e.1 <= at).checked_sub(1)?;
        let (id, first, count) = self.elements[k];
        (at < first + count).then_some(id)
    }
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds {
    pub fn empty() -> Self {
        Self { min: [f32::INFINITY; 3], max: [f32::NEG_INFINITY; 3] }
    }

    pub fn is_valid(&self) -> bool {
        (0..3).all(|i| self.min[i].is_finite() && self.max[i].is_finite() && self.min[i] <= self.max[i])
    }

    pub fn extend(&mut self, p: [f32; 3]) {
        for i in 0..3 {
            self.min[i] = self.min[i].min(p[i]);
            self.max[i] = self.max[i].max(p[i]);
        }
    }

    pub fn center(&self) -> [f32; 3] {
        [0, 1, 2].map(|i| 0.5 * (self.min[i] + self.max[i]))
    }

    pub fn size(&self) -> [f32; 3] {
        [0, 1, 2].map(|i| self.max[i] - self.min[i])
    }

    pub fn radius(&self) -> f32 {
        let s = self.size();
        0.5 * (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt()
    }
}

/// A loaded model.
#[derive(Debug)]
pub struct Model {
    /// Unique id, used by the GPU cache to know when to upload.
    pub id: u64,
    pub path: PathBuf,
    pub format: String,
    pub file_size: u64,
    pub load_time: Duration,
    pub layers: Vec<Layer>,
    pub bounds: Bounds,
    /// The file was Y up and the loader turned it to Z up.
    pub source_y_up: bool,
    /// Element properties (flat JSON object per element id).
    pub props: HashMap<u64, String>,
    /// Views saved in the file (Revit), in world coordinates.
    pub views: Vec<SavedView>,
    /// What the file says about the project besides geometry (Revit).
    pub project: ProjectData,
}

/// Project data besides geometry (Revit): project information, levels, phases, grids, rooms.
#[derive(Debug, Clone, Default)]
pub struct ProjectData {
    /// Project information: (key, value). Built-in keys: `number`, `name`, `client`, `status`;
    /// other keys are the captions of the project's own parameters.
    pub info: Vec<(String, String)>,
    /// Levels: name and elevation (metres), low to high.
    pub levels: Vec<(String, f32)>,
    /// Phases in the order of the project.
    pub phases: Vec<String>,
    pub grids: Vec<GridLine>,
    pub rooms: Vec<Room>,
}

/// A grid line (world, metres) at the height it is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct GridLine {
    pub name: String,
    pub a: [f32; 3],
    pub b: [f32; 3],
}

/// A room: number, name, level and the values of the project's own room parameters.
#[derive(Debug, Clone, Default)]
pub struct Room {
    pub number: String,
    pub name: String,
    pub level: Option<String>,
    pub extra: Vec<(String, String)>,
}

/// The kind of a saved view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    ThreeD,
    Plan,
    Section,
    Elevation,
    Detail,
}

/// Where a saved view looks from (world, Z up, metres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewCamera {
    pub eye: [f32; 3],
    /// Unit view direction, from the eye into the model.
    pub forward: [f32; 3],
    pub perspective: bool,
    /// Vertical field of view (radians), perspective views.
    pub fov_y: Option<f32>,
}

/// The heights a plan shows: from `low` up to the cut plane `high` (a ceiling plan looks up from
/// its cut plane: `from_below`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanRange {
    pub low: f32,
    pub high: f32,
    pub from_below: bool,
}

/// A view saved in the file, as the viewer can show it.
#[derive(Debug, Clone)]
pub struct SavedView {
    pub name: String,
    pub kind: ViewKind,
    pub camera: Option<ViewCamera>,
    /// What the view shows: a 3D view's section box or the crop of a section or elevation.
    pub clip: Option<crate::section::SectionBox>,
    pub plan: Option<PlanRange>,
    pub level: Option<String>,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl Model {
    /// Compose loaded files in the app, giving each source's element IDs a unique
    /// namespace. Original IDs remain in their source properties.
    pub fn combine(models: &[std::sync::Arc<Model>]) -> Self {
        let mut layers = Vec::new();
        let mut props = HashMap::new();
        let mut views = Vec::new();
        let mut project = ProjectData::default();
        let mut next_id = 1u64;
        for model in models {
            let source = model.file_name();
            let ids: std::collections::BTreeSet<_> = model.props.keys().copied()
                .chain(model.layers.iter().flat_map(|layer| layer.elements.iter().map(|e| e.0))).collect();
            let remap: HashMap<_, _> = ids.into_iter().map(|id| {
                let mapped = next_id;
                next_id += 1;
                (id, mapped)
            }).collect();
            for layer in &model.layers {
                let mut layer = layer.clone();
                layer.name = format!("{source} / {}", layer.name);
                for element in &mut layer.elements { element.0 = remap[&element.0]; }
                layers.push(layer);
            }
            props.extend(model.props.iter().map(|(id, value)| (remap[id], value.clone())));
            views.extend(model.views.iter().cloned().map(|mut view| {
                view.name = format!("{source} / {}", view.name);
                view
            }));
            project.info.extend(model.project.info.iter().map(|(key, value)| (format!("{source} / {key}"), value.clone())));
            project.levels.extend(model.project.levels.iter().cloned());
            project.phases.extend(model.project.phases.iter().cloned());
            project.grids.extend(model.project.grids.iter().cloned());
            project.rooms.extend(model.project.rooms.iter().cloned());
        }
        project.levels.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        project.levels.dedup();
        let mut combined = Self::new(format!("{} models", models.len()).into(), "Multiple models", layers);
        combined.props = props;
        combined.views = views;
        combined.project = project;
        combined.file_size = models.iter().map(|m| m.file_size).sum();
        combined.load_time = models.iter().map(|m| m.load_time).sum();
        combined
    }

    /// Builds a model from layers: drops empty layers and computes the bounds.
    pub fn new(path: PathBuf, format: impl Into<String>, mut layers: Vec<Layer>) -> Self {
        let props: HashMap<u64, String> = layers.iter_mut().flat_map(|l| std::mem::take(&mut l.props)).collect();
        layers.retain(|l| !l.indices.is_empty() && !l.positions.is_empty());
        layers.iter_mut().for_each(unshare_element_vertices);
        let layers: Vec<Layer> = layers.into_iter().flat_map(|l| split_layer(l, MAX_PART_VERTICES, MAX_PART_TRIANGLES)).collect();
        let mut bounds = Bounds::empty();
        for layer in &layers {
            for p in layer.positions.chunks_exact(3) {
                if p.iter().all(|v| v.is_finite()) {
                    bounds.extend([p[0], p[1], p[2]]);
                }
            }
        }
        let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            path,
            format: format.into(),
            file_size,
            load_time: Duration::ZERO,
            layers,
            bounds,
            source_y_up: false,
            props,
            views: Vec::new(),
            project: ProjectData::default(),
        }
    }

    pub fn file_name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    pub fn vertex_count(&self) -> usize {
        self.layers.iter().map(Layer::vertex_count).sum()
    }

    pub fn triangle_count(&self) -> usize {
        self.layers.iter().map(Layer::triangle_count).sum()
    }

    /// Where an element is drawn: (layer, first index, index count) per range.
    pub fn element_ranges(&self, id: u64) -> Vec<(usize, u32, u32)> {
        let mut out = Vec::new();
        for (i, layer) in self.layers.iter().enumerate() {
            out.extend(layer.elements.iter().filter(|e| e.0 == id).map(|e| (i, e.1, e.2)));
        }
        out
    }

    /// The properties of an element as (key, value) pairs in file order.
    pub fn element_props(&self, id: u64) -> Vec<(String, String)> {
        let Some(json) = self.props.get(&id) else { return Vec::new() };
        match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(json) {
            Ok(map) => map
                .into_iter()
                .map(|(k, v)| (k, v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())))
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// GPU buffer limits (wgpu default `max_buffer_size` is 256 MiB): at 32 bytes per vertex and up
/// to 6 line indices per triangle, parts stay below that.
pub const MAX_PART_VERTICES: usize = 8_000_000;
pub const MAX_PART_TRIANGLES: usize = 10_000_000;

/// Splits a layer that is too big for one GPU buffer into parts named "Name (part n)".
pub fn split_layer(layer: Layer, max_vertices: usize, max_triangles: usize) -> Vec<Layer> {
    if layer.vertex_count() <= max_vertices && layer.triangle_count() <= max_triangles {
        return vec![layer];
    }
    let mut parts = Vec::new();
    let mut remap: Vec<u32> = vec![u32::MAX; layer.vertex_count()];
    let mut part = empty_like(&layer);
    let mut used: Vec<u32> = Vec::new();
    // per source triangle: (part, first index in the part), for the element ranges
    let mut placed: Vec<Option<(usize, u32)>> = vec![None; layer.triangle_count()];
    for (t, tri) in layer.indices.chunks_exact(3).enumerate() {
        if tri.iter().any(|&i| i as usize >= remap.len()) {
            continue;
        }
        if part.vertex_count() + 3 > max_vertices || part.triangle_count() + 1 > max_triangles {
            for &u in &used {
                remap[u as usize] = u32::MAX;
            }
            used.clear();
            parts.push(std::mem::replace(&mut part, empty_like(&layer)));
        }
        placed[t] = Some((parts.len(), part.indices.len() as u32));
        for &i in tri {
            if remap[i as usize] == u32::MAX {
                remap[i as usize] = part.vertex_count() as u32;
                used.push(i);
                let s = 3 * i as usize;
                part.positions.extend_from_slice(&layer.positions[s..s + 3]);
                part.normals.extend_from_slice(layer.normals.get(s..s + 3).unwrap_or(&[0.0; 3]));
                if let (Some(dst), Some(src)) = (part.colors.as_mut(), layer.colors.as_ref()) {
                    dst.push(src.get(i as usize).copied().unwrap_or([255; 4]));
                }
            }
            part.indices.push(remap[i as usize]);
        }
    }
    if !part.indices.is_empty() {
        parts.push(part);
    }
    for &(id, first, count) in &layer.elements {
        let mut run: Option<(usize, u32, u32)> = None;
        for t in (first / 3)..(first + count) / 3 {
            let Some((p, at)) = placed.get(t as usize).copied().flatten() else { continue };
            match &mut run {
                Some((rp, rf, rc)) if *rp == p && *rf + *rc == at => *rc += 3,
                _ => {
                    if let Some((rp, rf, rc)) = run.take() {
                        parts[rp].elements.push((id, rf, rc));
                    }
                    run = Some((p, at, 3));
                }
            }
        }
        if let Some((rp, rf, rc)) = run {
            parts[rp].elements.push((id, rf, rc));
        }
    }
    let n = parts.len();
    for (k, p) in parts.iter_mut().enumerate() {
        p.name = format!("{} (part {}/{n})", layer.name, k + 1);
    }
    parts
}

fn empty_like(layer: &Layer) -> Layer {
    Layer {
        name: layer.name.clone(),
        color: layer.color,
        positions: Vec::new(),
        normals: Vec::new(),
        colors: layer.colors.as_ref().map(|_| Vec::new()),
        hidden: layer.hidden,
        ..Layer::default()
    }
}

/// Gives every element of a layer its own vertices: a vertex used by the triangles of two
/// elements (or of an element and of no element) is copied, so the exploded view can move an
/// element without dragging its neighbours along.
pub fn unshare_element_vertices(layer: &mut Layer) {
    if layer.elements.is_empty() {
        return;
    }
    // the owner of every triangle: 1 + the element's number in the layer, 0 outside elements
    let mut number: HashMap<u64, u32> = HashMap::new();
    let mut owner_of_triangle = vec![0u32; layer.triangle_count()];
    for &(id, first, count) in &layer.elements {
        let next = number.len() as u32 + 1;
        let owner = *number.entry(id).or_insert(next);
        let end = (((first + count) / 3) as usize).min(owner_of_triangle.len());
        if let Some(tris) = owner_of_triangle.get_mut(first as usize / 3..end) {
            tris.fill(owner);
        }
    }
    let n = layer.vertex_count();
    let mut owner_of_vertex = vec![u32::MAX; n];
    let mut copies: HashMap<(u32, u32), u32> = HashMap::new();
    let Layer { positions, normals, colors, indices, .. } = layer;
    for (k, index) in indices.iter_mut().enumerate() {
        let (v, Some(&owner)) = (*index as usize, owner_of_triangle.get(k / 3)) else { break };
        if v >= n || owner_of_vertex[v] == owner {
            continue;
        }
        if owner_of_vertex[v] == u32::MAX {
            owner_of_vertex[v] = owner;
            continue;
        }
        *index = *copies.entry((*index, owner)).or_insert_with(|| {
            let copy = (positions.len() / 3) as u32;
            if normals.len() == positions.len() {
                normals.extend_from_within(3 * v..3 * v + 3);
            }
            if let Some(c) = colors.as_mut().filter(|c| c.len() == copy as usize) {
                c.push(c[v]);
            }
            positions.extend_from_within(3 * v..3 * v + 3);
            copy
        });
    }
}

/// Converts Y-up coordinates (x, y, z) to Z-up (x, -z, y), in place, for xyz triples.
pub fn y_up_to_z_up(values: &mut [f32]) {
    for v in values.chunks_exact_mut(3) {
        let (y, z) = (v[1], v[2]);
        v[1] = -z;
        v[2] = y;
    }
}

/// Splits an indexed triangle mesh into unshared vertices with flat face normals.
pub fn flat_shaded(positions: &[f32], indices: &[u32]) -> (Vec<f32>, Vec<f32>, Vec<u32>) {
    let mut out_p = Vec::with_capacity(indices.len() * 3);
    let mut out_n = Vec::with_capacity(indices.len() * 3);
    let vertex_count = (positions.len() / 3) as u32;
    for tri in indices.chunks_exact(3) {
        if tri.iter().any(|&i| i >= vertex_count) {
            continue;
        }
        let p = [tri[0], tri[1], tri[2]].map(|i| {
            let i = i as usize * 3;
            glam::Vec3::new(positions[i], positions[i + 1], positions[i + 2])
        });
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        for v in p {
            out_p.extend_from_slice(&[v.x, v.y, v.z]);
            out_n.extend_from_slice(&[n.x, n.y, n.z]);
        }
    }
    let count = (out_p.len() / 3) as u32;
    (out_p, out_n, (0..count).collect())
}

/// Smooth vertex normals (area weighted) for an indexed mesh.
pub fn smooth_normals(positions: &[f32], indices: &[u32]) -> Vec<f32> {
    let n = positions.len() / 3;
    let mut normals = vec![glam::Vec3::ZERO; n];
    let at = |i: u32| {
        let i = i as usize * 3;
        glam::Vec3::new(positions[i], positions[i + 1], positions[i + 2])
    };
    for tri in indices.chunks_exact(3) {
        if tri.iter().any(|&i| i as usize >= n) {
            continue;
        }
        let face = (at(tri[1]) - at(tri[0])).cross(at(tri[2]) - at(tri[0]));
        for &i in tri {
            normals[i as usize] += face;
        }
    }
    normals.iter().flat_map(|v| v.normalize_or(glam::Vec3::Z).to_array()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn y_up_conversion() {
        let mut v = vec![1.0, 2.0, 3.0];
        y_up_to_z_up(&mut v);
        assert_eq!(v, vec![1.0, -3.0, 2.0]);
    }

    #[test]
    fn splits_big_layers() {
        let (positions, normals, indices) = flat_shaded(
            &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0],
            &[0, 1, 2, 0, 2, 3, 0, 1, 2],
        );
        let elements = vec![(7, 0, 6), (8, 6, 3)];
        let layer = Layer { name: "L".into(), color: [1.0; 3], positions, normals, indices, elements, ..Layer::default() };
        let parts = split_layer(layer, 6, 100);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].elements, vec![(7, 0, 6)]);
        assert_eq!(parts[1].elements, vec![(8, 0, 3)]);
        assert_eq!(parts[1].element_at(0), Some(8));
        assert_eq!(parts[0].element_at(1), Some(7));
        assert_eq!(parts[0].triangle_count(), 2);
        assert_eq!(parts[1].triangle_count(), 1);
        assert_eq!(parts[1].name, "L (part 2/2)");
        assert!(parts.iter().all(|p| p.indices.iter().all(|&i| (i as usize) < p.vertex_count())));
    }

    #[test]
    fn elements_get_their_own_vertices() {
        // two triangles sharing the edge 1-2: element 7, element 8; a third triangle outside both
        let positions = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0];
        let mut layer = Layer {
            normals: [0.0, 0.0, 1.0].repeat(4),
            colors: Some(vec![[1, 2, 3, 255], [4, 5, 6, 255], [7, 8, 9, 255], [0; 4]]),
            positions: positions.clone(),
            indices: vec![0, 1, 2, 1, 3, 2, 2, 3, 1],
            elements: vec![(7, 0, 3), (8, 3, 3)],
            ..Layer::default()
        };
        unshare_element_vertices(&mut layer);
        // element 8 gets copies of 1 and 2; the free triangle its own copies of 2, 3 and 1
        assert_eq!(layer.indices, vec![0, 1, 2, 4, 3, 5, 6, 7, 8]);
        assert_eq!(layer.vertex_count(), 9);
        assert_eq!(layer.normals.len(), layer.positions.len());
        assert_eq!(&layer.positions[12..18], &positions[3..9]);
        assert_eq!(layer.colors.as_ref().map(|c| c[5]), Some([7, 8, 9, 255]));
        // nothing shared: unchanged
        let before = layer.clone();
        unshare_element_vertices(&mut layer);
        assert_eq!((layer.indices, layer.positions), (before.indices, before.positions));
    }

    #[test]
    fn sheets_do_not_enclose_volume() {
        // a closed tetrahedron, one face wound the other way
        let p = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        assert!(encloses_volume(&p, &[0, 2, 1, 0, 1, 3, 1, 2, 3, 0, 2, 3]));
        // a single sheet of two triangles
        let q = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0];
        assert!(!encloses_volume(&q, &[0, 1, 2, 0, 2, 3]));
        let layer = Layer { positions: q.to_vec(), indices: vec![0, 1, 2, 0, 2, 3], ..Layer::default() };
        assert!(layer.solid_indices().is_empty());
    }

    #[test]
    fn flat_normals_of_xy_triangle_point_up() {
        let (p, n, i) = flat_shaded(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], &[0, 1, 2]);
        assert_eq!(p.len(), 9);
        assert_eq!(i, vec![0, 1, 2]);
        assert_eq!(&n[0..3], &[0.0, 0.0, 1.0]);
    }
}

/// Whether a set of triangles encloses volume, for the cut fill of the section box. Only open
/// sheets (terrain, leaves, entourage) are refused: they make the fill's surface count odd outside
/// any solid. Seams with T-junctions and faces wound the other way do not disturb the count, so a
/// set passes when either test says closed: the area vectors nearly cancel (racbasicsample: walls
/// at most 0.10, mullions 0.22 with some faces wound the other way; terrain 0.98, planting 0.65),
/// or almost every welded edge is paired (at most 2 %; a large terrain sheet leaves only its
/// border open, 12 %).
pub fn encloses_volume(positions: &[f32], indices: &[u32]) -> bool {
    if openness(positions, indices) <= 0.3 {
        return true;
    }
    let (open, all) = open_edges(positions, indices);
    open as f32 <= 0.02 * all as f32
}

/// How far a set of triangles (index triples into `positions`) is from enclosing a volume: the
/// length of the sum of the area vectors over the sum of the areas. A closed, consistently wound
/// surface sums to zero (divergence theorem), T-junctions or not; an open sheet such as terrain
/// is near 1.
pub fn openness(positions: &[f32], indices: &[u32]) -> f32 {
    let at = |i: u32| {
        let i = 3 * i as usize;
        glam::DVec3::new(positions[i] as f64, positions[i + 1] as f64, positions[i + 2] as f64)
    };
    let (mut sum, mut area) = (glam::DVec3::ZERO, 0.0);
    for tri in indices.chunks_exact(3) {
        let (a, b, c) = (at(tri[0]), at(tri[1]), at(tri[2]));
        let n = (b - a).cross(c - a);
        sum += n;
        area += n.length();
    }
    if area <= 0.0 { 0.0 } else { (sum.length() / area) as f32 }
}

/// Open edges and all edges of a set of triangles (index triples into `positions`), with the
/// vertices welded by position: a closed surface uses every edge an even number of times.
pub fn open_edges(positions: &[f32], indices: &[u32]) -> (usize, usize) {
    use std::collections::HashMap;
    let key = |i: u32| -> [i32; 3] {
        let i = 3 * i as usize;
        [0, 1, 2].map(|k| (positions[i + k] * 1e4).round() as i32)
    };
    let mut edges: HashMap<([i32; 3], [i32; 3]), u32> = HashMap::with_capacity(indices.len());
    for tri in indices.chunks_exact(3) {
        let k = [key(tri[0]), key(tri[1]), key(tri[2])];
        for (a, b) in [(k[0], k[1]), (k[1], k[2]), (k[2], k[0])] {
            if a != b {
                *edges.entry(if a < b { (a, b) } else { (b, a) }).or_default() += 1;
            }
        }
    }
    (edges.values().filter(|&&n| n % 2 == 1).count(), edges.len())
}
