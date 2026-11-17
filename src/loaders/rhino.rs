//! Rhino 3D models (`.3dm`), Rhino 2 to 9. The file is read by the `threedm-read` crate
//! ([3dm-dll](https://github.com/OpenAEC-Foundation/3dm-dll)): its layers, materials, block
//! definitions and objects as meshes (exact geometry, SubD, texts and filled hatches meshed as
//! Rhino meshes them), polylines (curves and hatch lines), points and block instances. This
//! loader draws that model.
//!
//! One layer per Rhino layer (its full path, `Parent::Child`), in the layer colour, hidden when
//! the layer or one of its parents is off; the members of a block instance on a layer that is
//! off go to that (hidden) layer. Block instances with their nested blocks and transforms.
//! Every object is a pickable element with its name, object type, layer, id, material and user
//! text; a block instance is one element. Objects take their display colour by layer, object or
//! parent (blocks), or their material's diffuse colour and transparency when they have a
//! material; black (Rhino's default layer colour) without a material draws in the viewer's
//! grey; Brep face colours (Rhino 7) and mesh vertex colours are kept. Curves draw as lines.
//! Units are converted to metres; Rhino is Z up; a model far from zero (survey coordinates)
//! moves to a local origin, kept in the project information. Point clouds are thinned out while
//! drawing; blocks, meshes and points are drawn up to limits, so that a damaged or hostile file
//! cannot exhaust memory or run for ever. What is left out (annotations other than texts, text
//! dots, lights, hidden objects, objects on layouts) is counted in the project information.

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};
use glam::{DMat3, DMat4, DVec3};
use std::collections::{BTreeMap, HashMap, HashSet};
use threedm_read::{Attributes, File, Geometry, Material, Mesh, RhinoLayer, Uuid};

pub struct RhinoLoader;

impl Loader for RhinoLoader {
    fn name(&self) -> &'static str {
        "Rhino"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["3dm"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|l| l.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        let file = threedm_read::read_with(&data, &threedm_read::Options { font: Some(crate::INTER_REGULAR) })?;
        progress.set(Phase::Convert);
        let (layers, info) = convert(&file);
        if layers.iter().all(|l| l.indices.is_empty() && l.lines.is_empty() && !(l.points && !l.positions.is_empty())) {
            // say why, e.g. polysurfaces saved without their render meshes
            let left_out = info.iter().find(|(k, _)| k == "Rhino objects left out").map_or("no objects", |(_, v)| v.as_str());
            return Err(format!("{}: nothing to draw in this Rhino file ({left_out})", source.name()));
        }
        Ok(Loaded { structural: Vec::new(), origin: None, layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } })
    }
}


// ---------------------------------------------------------------------------------------------
// From the file to layers: one layer per Rhino layer, objects and block instances as elements.

/// Block instances nested deeper than this are left out.
const MAX_BLOCK_DEPTH: usize = 32;

/// At most this many objects are drawn through block instances (every member of every placed
/// instance counts): nested blocks multiply, and a damaged or hostile file could otherwise keep
/// them going for ever. Smaller in the (unoptimised) tests, which run into it.
const MAX_BLOCK_PARTS: usize = if cfg!(test) { 100_000 } else { 10_000_000 };

/// At most this many triangles are drawn, block instances included, and at most three vertices
/// per triangle, so that indices stay well within a u32. The browser has less memory.
const MAX_TRIANGLES: usize = if cfg!(target_arch = "wasm32") { 8_000_000 } else { 40_000_000 };
const MAX_DRAWN_VERTICES: usize = 3 * MAX_TRIANGLES;

struct Scene<'f> {
    file: &'f File<'f>,
    /// Rhino layer by the index objects use.
    layers: HashMap<i32, usize>,
    /// Rhino layer by id, for the parents.
    layer_ids: HashMap<Uuid, usize>,
    /// The full path of a Rhino layer and whether it and its parents are on, worked out once.
    lineages: HashMap<usize, (String, bool)>,
    materials: HashMap<i32, usize>,
    blocks: HashMap<Uuid, usize>,
    objects: HashMap<Uuid, usize>,
    /// Viewer layers: meshes and points per Rhino layer (`None`: an object without a layer).
    meshes: BTreeMap<Option<usize>, Layer>,
    points: BTreeMap<Option<usize>, Layer>,
    next_id: u64,
    /// The block definitions being drawn: one met again contains itself.
    expanding: HashSet<Uuid>,
    /// Block members drawn so far, and whether the current instance was cut short by
    /// [`MAX_BLOCK_PARTS`].
    parts: usize,
    cut: bool,
    /// Triangles and vertices drawn so far.
    triangles: usize,
    vertices: usize,
    /// The points of the file; every `stride`-th point met is drawn, at most `MAX_POINTS`.
    total_points: usize,
    stride: u64,
    seen: u64,
    kept: usize,
    /// The local origin (metres) taken off everything drawn.
    origin: Option<DVec3>,
    /// What was drawn and what was left out, by kind.
    drawn: BTreeMap<&'static str, usize>,
    skipped: BTreeMap<&'static str, usize>,
    /// The objects drawn with meshes the viewer made (by the address of their meshes).
    meshed: HashSet<usize>,
}

/// The colour an object is drawn in: sRGB and alpha.
type Rgba = [u8; 4];

fn opaque(c: [u8; 4]) -> Rgba {
    [c[0], c[1], c[2], 255]
}

/// How an object is coloured: its display colour (by layer, object or parent) and its material.
#[derive(Clone, Copy)]
struct Paint<'f> {
    color: Rgba,
    material: Option<&'f Material>,
}

impl Paint<'_> {
    /// The colour drawn: the material's diffuse colour and transparency when there is a
    /// material, else the display colour.
    fn rgba(&self) -> Rgba {
        match self.material {
            Some(m) => {
                let alpha = (255.0 * (1.0 - m.transparency.clamp(0.0, 1.0))).round() as u8;
                [m.diffuse[0], m.diffuse[1], m.diffuse[2], alpha]
            }
            None => display(self.color),
        }
    }
}

/// Where the members of a block instance are drawn.
#[derive(Clone, Copy)]
struct Placement<'f> {
    xform: DMat4,
    /// The instance's colour and material, for members by parent.
    parent: Paint<'f>,
    /// The Rhino layer of the outermost instance on a layer that is off: Rhino hides the whole
    /// instance, so its members go to that (hidden) layer instead of their own.
    off: Option<i32>,
}

impl<'f> Scene<'f> {
    fn new(file: &'f File<'f>) -> Self {
        let index = |n: usize, key: &dyn Fn(usize) -> i32| -> HashMap<i32, usize> {
            let mut map = HashMap::new();
            for k in 0..n {
                map.entry(key(k)).or_insert(k);
            }
            map
        };
        let mut layer_ids = HashMap::new();
        for (k, l) in file.layers.iter().enumerate().filter(|(_, l)| !l.id.is_nil()) {
            layer_ids.entry(l.id).or_insert(k);
        }
        let total_points: usize = file.objects.iter().map(|o| if let Geometry::Points { points, .. } = &o.geometry { points.len() / 24 } else { 0 }).sum();
        Scene {
            file,
            layers: index(file.layers.len(), &|k| file.layers[k].index),
            layer_ids,
            lineages: HashMap::new(),
            materials: index(file.materials.len(), &|k| file.materials[k].index),
            blocks: file.blocks.iter().enumerate().map(|(k, b)| (b.id, k)).collect(),
            objects: file.objects.iter().enumerate().filter(|(_, o)| !o.attributes.id.is_nil()).map(|(k, o)| (o.attributes.id, k)).collect(),
            meshes: BTreeMap::new(),
            points: BTreeMap::new(),
            next_id: 1,
            expanding: HashSet::new(),
            parts: 0,
            cut: false,
            triangles: 0,
            vertices: 0,
            total_points,
            stride: total_points.div_ceil(super::pointcloud::MAX_POINTS).max(1) as u64,
            seen: 0,
            kept: 0,
            origin: None,
            drawn: BTreeMap::new(),
            skipped: BTreeMap::new(),
            meshed: HashSet::new(),
        }
    }

    fn layer(&self, index: i32) -> Option<&'f RhinoLayer> {
        self.layers.get(&index).map(|&k| &self.file.layers[k])
    }

    fn material(&self, index: i32) -> Option<&'f Material> {
        self.materials.get(&index).map(|&k| &self.file.materials[k])
    }

    /// The full path of Rhino layer `k` (`Parent::Child`) and whether it and all its parents are
    /// on. Parents are followed up to 64 levels, and a cycle of parents stops at the first layer
    /// met again.
    fn lineage(&mut self, k: usize) -> &(String, bool) {
        if !self.lineages.contains_key(&k) {
            let layers = &self.file.layers;
            let mut chain = vec![k];
            while chain.len() < 64 {
                let parent = layers[chain[chain.len() - 1]].parent;
                match self.layer_ids.get(&parent) {
                    Some(&p) if !chain.contains(&p) => chain.push(p),
                    _ => break,
                }
            }
            let path = chain.iter().rev().map(|&p| layers[p].name.as_str()).collect::<Vec<_>>().join("::");
            let shown = chain.iter().all(|&p| layers[p].visible);
            self.lineages.insert(k, (path, shown));
        }
        &self.lineages[&k]
    }

    /// Whether the layer of index `index` and all its parents are on.
    fn layer_shown(&mut self, index: i32) -> bool {
        match self.layers.get(&index) {
            Some(&k) => self.lineage(k).1,
            None => true,
        }
    }

    fn viewer_layer(&mut self, layer: i32, points: bool) -> &mut Layer {
        let key = self.layers.get(&layer).copied();
        if (if points { &self.points } else { &self.meshes }).contains_key(&key) {
            let map = if points { &mut self.points } else { &mut self.meshes };
            return map.get_mut(&key).unwrap();
        }
        let (name, color, hidden) = match key {
            Some(k) => {
                let color = display(opaque(self.file.layers[k].color));
                let (path, shown) = self.lineage(k).clone();
                (path, color, !shown)
            }
            None => ("Default".to_string(), display([0, 0, 0, 255]), false),
        };
        let map = if points { &mut self.points } else { &mut self.meshes };
        map.entry(key).or_insert_with(|| Layer {
            name: if points { format!("{name} (points)") } else { name },
            color: [color[0] as f32 / 255.0, color[1] as f32 / 255.0, color[2] as f32 / 255.0],
            hidden,
            points,
            colors: Some(Vec::new()),
            ..Layer::default()
        })
    }

    /// The colour and material of an object; `parent` those of the block instance it is part
    /// of. Colour and material each come by layer, object or parent, independently; by parent
    /// outside a block is by layer, as in Rhino.
    fn paint(&self, a: &Attributes, parent: Option<Paint<'f>>) -> Paint<'f> {
        let layer = self.layer(a.layer);
        let material = match (a.material_source, parent) {
            (1, _) => self.material(a.material),
            (3, Some(p)) => p.material,
            _ => layer.and_then(|l| self.material(l.material)),
        };
        let layer_color = layer.map(|l| opaque(l.color)).unwrap_or([0, 0, 0, 255]);
        let color = match (a.color_source, parent) {
            (1, _) => opaque(a.color),
            (3, Some(p)) => p.color,
            _ => layer_color,
        };
        Paint { color, material }
    }

    /// Element properties: name, object type, layer, id, material, block, user text.
    fn props(&mut self, a: &Attributes, kind: &str, block: Option<&str>, user: &[(String, String)]) -> String {
        let mut p = serde_json::Map::new();
        if !a.name.is_empty() {
            p.insert("Name".into(), a.name.clone().into());
        } else if let Some(b) = block {
            p.insert("Name".into(), b.into());
        }
        p.insert("Type".into(), kind.into());
        if let Some(b) = block {
            p.insert("Block".into(), b.into());
        }
        if let Some(&k) = self.layers.get(&a.layer) {
            p.insert("Layer".into(), self.lineage(k).0.clone().into());
        }
        if let Some(m) = self.paint(a, None).material.filter(|m| !m.name.is_empty()) {
            p.insert("Material".into(), m.name.clone().into());
        }
        if !a.id.is_nil() {
            p.insert("Id".into(), a.id.to_string().into());
        }
        for (key, value) in user {
            let key = if p.contains_key(key) { format!("{key} (user text)") } else { key.clone() };
            p.insert(key, value.clone().into());
        }
        serde_json::Value::Object(p).to_string()
    }

    fn run(&mut self) {
        let root = DMat4::from_scale(DVec3::splat(self.file.meters));
        let file = self.file;
        for object in &file.objects {
            let a = &object.attributes;
            if a.mode == 3 {
                continue; // block definition geometry, drawn by its instances
            }
            if a.space == 2 {
                *self.skipped.entry("objects on layouts").or_default() += 1;
                continue;
            }
            if !a.visible || a.mode == 1 {
                *self.skipped.entry("hidden objects").or_default() += 1;
                continue;
            }
            let id = self.next_id;
            self.next_id += 1;
            match &object.geometry {
                Geometry::Skipped(why) => *self.skipped.entry(why).or_default() += 1,
                Geometry::Instance { definition, xform } => {
                    let name = self.blocks.get(definition).map(|&k| file.blocks[k].name.clone()).unwrap_or_default();
                    let props = self.props(a, "Block instance", Some(&name), &object.user);
                    let off = (!self.layer_shown(a.layer)).then_some(a.layer);
                    let at = Placement { xform: root * *xform, parent: self.paint(a, None), off };
                    let mut first = Some(props);
                    self.cut = false;
                    let drawn = self.instance(*definition, at, id, &mut first, 1);
                    if self.cut {
                        *self.skipped.entry("block instances cut short (too many parts)").or_default() += 1;
                    }
                    *if drawn { self.drawn.entry("block instances") } else { self.skipped.entry("empty block instances") }.or_default() += 1;
                }
                geometry => {
                    let kind = match geometry {
                        Geometry::Meshes(_, kind) | Geometry::Points { kind, .. } | Geometry::Lines(_, kind) | Geometry::Drawing { kind, .. } => *kind,
                        _ => "",
                    };
                    let props = self.props(a, kind, None, &object.user);
                    let color = self.paint(a, None).rgba();
                    let mut first = Some(props);
                    if self.draw(geometry, a.layer, root, color, id, &mut first) {
                        *self.drawn.entry(plural(kind)).or_default() += 1;
                    }
                }
            }
        }
    }

    /// Draws the objects of a block definition, placed by `at`, as part of element `id`.
    fn instance(&mut self, definition: Uuid, at: Placement<'f>, id: u64, props: &mut Option<String>, depth: usize) -> bool {
        if depth > MAX_BLOCK_DEPTH {
            *self.skipped.entry("blocks nested too deep").or_default() += 1;
            return false;
        }
        let Some(&k) = self.blocks.get(&definition) else { return false };
        if !self.expanding.insert(definition) {
            *self.skipped.entry("blocks that contain themselves").or_default() += 1;
            return false;
        }
        let file = self.file;
        let mut drawn = false;
        for uuid in &file.blocks[k].objects {
            if self.parts >= MAX_BLOCK_PARTS {
                self.cut = true;
                break;
            }
            self.parts += 1;
            let Some(&o) = self.objects.get(uuid) else { continue };
            let object = &file.objects[o];
            let a = &object.attributes;
            if !a.visible {
                continue;
            }
            let paint = self.paint(a, Some(at.parent));
            drawn |= match &object.geometry {
                Geometry::Instance { definition, xform: inner } => {
                    let off = at.off.or_else(|| (!self.layer_shown(a.layer)).then_some(a.layer));
                    self.instance(*definition, Placement { xform: at.xform * *inner, parent: paint, off }, id, props, depth + 1)
                }
                geometry => self.draw(geometry, at.off.unwrap_or(a.layer), at.xform, paint.rgba(), id, props),
            };
        }
        self.expanding.remove(&definition);
        drawn
    }

    /// The local origin, taken from the first point drawn (`first`): that point in whole metres
    /// when it is far from zero, as the point cloud readers do, so that survey coordinates keep
    /// their precision in f32.
    fn origin(&mut self, first: Option<DVec3>) -> DVec3 {
        match first {
            Some(p) => *self.origin.get_or_insert_with(|| DVec3::from(super::pointcloud::origin_for(p.to_array()))),
            None => self.origin.unwrap_or(DVec3::ZERO),
        }
    }

    /// Adds meshes or points to the layer of `layer`; the first part drawn carries the props.
    fn draw(&mut self, geometry: &Geometry<'_>, layer: i32, xform: DMat4, color: Rgba, id: u64, props: &mut Option<String>) -> bool {
        let color = ink(geometry, color);
        match geometry {
            Geometry::Meshes(meshes, _) => {
                // the meshes within the triangle and vertex limits
                let fits: Vec<&Mesh> = meshes
                    .iter()
                    .filter(|m| {
                        let vertices = if m.normals.is_some() { m.positions.len() } else { 3 * m.triangles };
                        let fits = self.triangles + m.triangles <= MAX_TRIANGLES && self.vertices + vertices <= MAX_DRAWN_VERTICES;
                        if fits {
                            self.triangles += m.triangles;
                            self.vertices += vertices;
                        }
                        fits
                    })
                    .collect();
                if fits.len() < meshes.len() {
                    *self.skipped.entry("objects beyond the triangle limit").or_default() += 1;
                }
                let first = fits.iter().flat_map(|m| &m.positions).map(|p| xform.transform_point3(DVec3::from(p.map(f64::from)))).find(|p| p.is_finite());
                let origin = self.origin(first);
                let target = self.viewer_layer(layer, false);
                let start = target.indices.len() as u32;
                for mesh in fits {
                    add_mesh(target, mesh, xform, origin, color);
                }
                let count = target.indices.len() as u32 - start;
                if count == 0 {
                    return false;
                }
                target.elements.push((id, start, count));
                if let Some(p) = props.take() {
                    target.props.push((id, p));
                }
                if meshes.iter().any(|m| m.computed) {
                    self.meshed.insert(meshes.as_ptr() as usize);
                }
                true
            }
            Geometry::Lines(polylines, _) => {
                let first = polylines.iter().flatten().map(|p| xform.transform_point3(*p)).find(|p| p.is_finite());
                let origin = self.origin(first);
                let target = self.viewer_layer(layer, false);
                let before = target.lines.len();
                for line in polylines {
                    let points: Vec<DVec3> = line.iter().map(|p| xform.transform_point3(*p) - origin).collect();
                    if points.len() < 2 || points.iter().any(|p| !p.is_finite()) {
                        continue;
                    }
                    let base = (target.positions.len() / 3) as u32;
                    for p in &points {
                        target.positions.extend(p.to_array().map(|v| v as f32));
                        target.normals.extend([0.0; 3]);
                    }
                    target.colors.get_or_insert_with(Vec::new).extend(std::iter::repeat_n(color, points.len()));
                    target.lines.extend((0..points.len() as u32 - 1).flat_map(|k| [base + k, base + k + 1]));
                }
                let count = (target.lines.len() - before) as u32;
                if count == 0 {
                    return false;
                }
                target.line_elements.push((id, before as u32, count));
                if let Some(p) = props.take() {
                    target.props.push((id, p));
                }
                true
            }
            Geometry::Points { points, colors, .. } => {
                let count = points.len() / 24;
                if count == 0 {
                    return false;
                }
                if self.kept >= super::pointcloud::MAX_POINTS {
                    *self.skipped.entry("point objects beyond the point limit").or_default() += 1;
                    return false;
                }
                let point = |k: usize| {
                    let f = |i: usize| f64::from_le_bytes(points[24 * k + 8 * i..24 * k + 8 * i + 8].try_into().unwrap());
                    xform.transform_point3(DVec3::new(f(0), f(1), f(2)))
                };
                let origin = self.origin((0..count).map(point).find(|p| p.is_finite()));
                // thinned out while drawing: only every stride-th point of the file is converted
                let (stride, mut seen, mut kept) = (self.stride, self.seen, self.kept);
                let target = self.viewer_layer(layer, true);
                for k in 0..count {
                    let keep = seen % stride == 0;
                    seen += 1;
                    if !keep {
                        continue;
                    }
                    if kept >= super::pointcloud::MAX_POINTS {
                        break;
                    }
                    let p = point(k);
                    if !p.is_finite() {
                        continue;
                    }
                    target.positions.extend((p - origin).to_array().map(|v| v as f32));
                    let c = colors.map_or(color, |c| [c[4 * k], c[4 * k + 1], c[4 * k + 2], 255]);
                    target.colors.get_or_insert_with(Vec::new).push(c);
                    kept += 1;
                }
                (self.seen, self.kept) = (seen, kept);
                true
            }
            Geometry::Drawing { lines, meshes, kind } => {
                // dimensions and leaders: their lines, and their arrowheads and text, as one element
                let drew_lines = self.draw(&Geometry::Lines(lines.clone(), kind), layer, xform, color, id, props);
                let drew_meshes = self.draw(&Geometry::Meshes(meshes.clone(), kind), layer, xform, color, id, props);
                drew_lines || drew_meshes
            }
            _ => false,
        }
    }

    fn finish(mut self) -> (Vec<Layer>, Vec<(String, String)>) {
        let mut layers: Vec<Layer> = std::mem::take(&mut self.meshes).into_values().chain(std::mem::take(&mut self.points).into_values()).collect();
        for layer in &mut layers {
            // one colour for the whole layer: no vertex colours
            let base = [0, 1, 2].map(|k| (layer.color[k] * 255.0).round() as u8);
            if layer.colors.as_ref().is_some_and(|c| c.iter().all(|c| c[..3] == base && c[3] == 255)) {
                layer.colors = None;
            }
        }
        let file = self.file;
        let mut info = vec![("Rhino file version".to_string(), version_name(file.versions.archive))];
        if !file.application.is_empty() {
            info.push(("Rhino application".into(), file.application.clone()));
        }
        info.push(("Rhino units".into(), if file.meters == 1.0 { file.unit.to_string() } else { format!("{} (converted to metres)", file.unit) }));
        if let Some(o) = self.origin.filter(|o| *o != DVec3::ZERO) {
            info.push(("Rhino world origin (metres)".into(), format!("{}, {}, {}", o.x, o.y, o.z)));
        }
        let list = |m: &BTreeMap<&str, usize>| m.iter().map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(", ");
        if !self.drawn.is_empty() {
            info.push(("Rhino objects drawn".into(), list(&self.drawn)));
        }
        if !self.meshed.is_empty() {
            info.push(("Rhino objects meshed by the viewer".into(), format!("{} (saved without a render mesh)", self.meshed.len())));
        }
        if self.stride > 1 {
            info.push(("Rhino points".into(), format!("{} in the file, every {}th drawn", self.total_points, self.stride)));
        }
        if !self.skipped.is_empty() {
            info.push(("Rhino objects left out".into(), list(&self.skipped)));
        }
        if !file.damaged.is_empty() {
            info.push(("Rhino unreadable records".into(), list(&file.damaged)));
        }
        (layers, info)
    }
}

/// Black, Rhino's colour for the default layer, draws in the viewer's grey.
fn display(c: Rgba) -> Rgba {
    if c[..3] == [0, 0, 0] {
        let g = DEFAULT_COLOR.map(|v| (v * 255.0).round() as u8);
        [g[0], g[1], g[2], c[3]]
    } else {
        c
    }
}

/// Curves, texts, dimensions and leaders in black (Rhino's default colour) draw dark, as ink,
/// rather than in the viewer's grey for surfaces, which hardly shows lines and letters.
fn ink(geometry: &Geometry<'_>, color: Rgba) -> Rgba {
    let drafting = match geometry {
        Geometry::Lines(..) | Geometry::Drawing { .. } => true,
        Geometry::Meshes(_, kind) => *kind == "Text",
        _ => false,
    };
    if drafting && color[..3] == display([0, 0, 0, 255])[..3] {
        [38, 38, 38, color[3]]
    } else {
        color
    }
}

fn plural(kind: &str) -> &'static str {
    match kind {
        "Mesh" => "meshes",
        "Surface" => "surfaces",
        "Polysurface" => "polysurfaces",
        "Extrusion" => "extrusions",
        "Point cloud" => "point clouds",
        "Point" => "points",
        "SubD" => "SubD objects",
        "Curve" => "curves",
        "Hatch" => "hatches",
        "Text" => "texts",
        "Dimension" => "dimensions",
        "Leader" => "leaders",
        "Center mark" => "center marks",
        "Text dot" => "text dots",
        _ => "objects",
    }
}

fn version_name(archive: u32) -> String {
    let rhino = match archive {
        2..=5 => archive,
        v => v / 10,
    };
    format!("Rhino {rhino} (archive version {archive})")
}

/// Adds the triangles of a mesh, transformed and moved by `-origin`, with its normals (or flat
/// ones) and colours.
fn add_mesh(layer: &mut Layer, mesh: &Mesh, xform: DMat4, origin: DVec3, color: Rgba) {
    let n = mesh.positions.len() as u32;
    let normal_matrix = DMat3::from_mat4(xform).inverse().transpose();
    let mirrored = xform.determinant() < 0.0;
    let color = mesh.color.map_or(color, opaque);
    // in f64 up to the local origin, so that survey coordinates keep their precision
    let point = |i: u32| xform.transform_point3(DVec3::from(mesh.positions[i as usize].map(f64::from))) - origin;
    let vertex_color = |i: u32| mesh.colors.as_ref().map_or(color, |c| {
        let c = c[i as usize];
        [c[0], c[1], c[2], 255 - c[3]]
    });
    let colors = layer.colors.get_or_insert_with(Vec::new);
    let mut triangles = Vec::with_capacity(2 * mesh.faces.len());
    for f in &mesh.faces {
        if f.iter().any(|&i| i >= n) {
            continue;
        }
        if f[2] == f[3] {
            triangles.push([f[0], f[1], f[2]]);
            continue;
        }
        // a quad splits along its shorter diagonal, as Rhino draws it (a bent quad differs)
        let d = |a: u32, b: u32| (DVec3::from(mesh.positions[a as usize].map(f64::from)) - DVec3::from(mesh.positions[b as usize].map(f64::from))).length_squared();
        if d(f[0], f[2]) <= d(f[1], f[3]) {
            triangles.extend([[f[0], f[1], f[2]], [f[0], f[2], f[3]]]);
        } else {
            triangles.extend([[f[0], f[1], f[3]], [f[1], f[2], f[3]]]);
        }
    }
    if let Some(normals) = &mesh.normals {
        // shared vertices with the file's normals
        let base = layer.positions.len() as u32 / 3;
        for i in 0..n {
            layer.positions.extend(point(i).to_array().map(|v| v as f32));
            // the inverse transpose keeps normals outward under a mirroring transform too
            let normal = (normal_matrix * DVec3::from(normals[i as usize].map(f64::from))).normalize_or_zero();
            layer.normals.extend(normal.to_array().map(|v| v as f32));
            colors.push(vertex_color(i));
        }
        for t in triangles {
            let (b, c) = if mirrored { (t[2], t[1]) } else { (t[1], t[2]) };
            layer.indices.extend([base + t[0], base + b, base + c]);
        }
    } else {
        // flat shaded
        for t in triangles {
            let order = if mirrored { [0, 2, 1] } else { [0, 1, 2] };
            let p = order.map(|k| point(t[k]));
            let normal = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            let base = layer.positions.len() as u32 / 3;
            for (k, &corner) in order.iter().enumerate() {
                layer.positions.extend(p[k].to_array().map(|v| v as f32));
                layer.normals.extend(normal.to_array().map(|v| v as f32));
                colors.push(vertex_color(t[corner]));
            }
            layer.indices.extend([base, base + 1, base + 2]);
        }
    }
}

fn convert(file: &File<'_>) -> (Vec<Layer>, Vec<(String, String)>) {
    let mut scene = Scene::new(file);
    scene.run();
    scene.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use threedm_core::write::*;
    use threedm_core::*;


    fn load(name: &str, data: Vec<u8>) -> (crate::model::Model, Vec<(String, String)>) {
        let source = Source::Bytes { name: name.into(), data: Arc::new(data) };
        let model = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("3dm");
        let info = model.project.info.clone();
        (model, info)
    }

    fn entry<'a>(info: &'a [(String, String)], key: &str) -> &'a str {
        info.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).unwrap_or("")
    }

    const QUAD: [[f32; 3]; 4] = [[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [1000.0, 2000.0, 0.0], [0.0, 2000.0, 0.0]];
    const UP: [[f32; 3]; 4] = [[0.0, 0.0, 1.0]; 4];

    /// A Rhino 7 file in millimetres: layers with a parent and a hidden one, a material, a mesh
    /// with user text, a polysurface with render meshes, an extrusion with a mesh cache, a block
    /// instance, a point cloud, and objects that are left out.
    fn rhino7() -> Vec<u8> {
        let w = W { big: true };
        let layers = w.table(TCODE_LAYER_TABLE, &[
            w.layer(0, "Walls", [200, 0, 0, 0], true, id(1), Uuid::default(), -1),
            w.layer(1, "Glass", [0, 0, 0, 0], true, id(2), id(1), 0),
            w.layer(2, "Hidden", [0, 200, 0, 0], false, id(3), Uuid::default(), -1),
            w.layer(3, "Off parent child", [0, 0, 200, 0], true, id(4), id(3), -1),
        ]);
        let glass = cat(&[
            &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&0x0Cu32.to_le_bytes(), &i32b(0), &wstr("Blue glass")])),
            &[0; 16],
            &[0, 0, 0, 0],
            &[0, 0, 255, 0],
            &[0; 16],
            &f64s(&[1.0, 0.0, 0.0, 0.5]),
        ]);
        let materials = w.table(TCODE_MATERIAL_TABLE, &[w.chunk(TCODE_MATERIAL_RECORD, &w.object(ON_MATERIAL, &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &glass), &[]))]);
        let model_attributes = w.versioned(TCODE_MODEL_ATTRIBUTES_CHUNK, 1, 0, &cat(&[&[0], &[1], &id(40).0, &[0], &[0], &[1], &wstr("Chair")]));
        let units = w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&i32b(2), &f64s(&[0.001]), &wstr("")]));
        let definition = cat(&[&model_attributes, &i32b(1), &units, &wstr(""), &wstr(""), &wstr(""), &f64s(&[0.0; 6]), &[1], &i32b(1), &id(41).0, &[0]]);
        let blocks = w.table(TCODE_INSTANCE_DEFINITION_TABLE, &[w.chunk(TCODE_INSTANCE_DEFINITION_RECORD, &w.object(ON_INSTANCE_DEFINITION, &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &definition), &[]))]);

        let quad = [[0, 1, 2, 3]];
        let slab = w.object(ON_MESH, &w.mesh(&QUAD, &quad, Some(&UP), None, true), &[]);
        let triangle = |z: f32| w.mesh(&[[0.0, 0.0, z], [1000.0, 0.0, z], [0.0, 1000.0, z]], &[[0, 1, 2, 2]], None, None, false);
        let brep = w.object(BREP_ALIASES[0], &w.brep(&[triangle(3000.0), triangle(3500.0)]), &[]);
        // an extrusion (Rhino 6 layout) whose mesh cache holds an analysis mesh, then the render mesh
        let item = |cache: &mut Vec<u8>, id: Uuid, z: f32| {
            cache.push(1);
            cache.extend(w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&id.0, &w.object(ON_MESH, &triangle(z), &[])])));
        };
        let mut cache = Vec::new();
        item(&mut cache, Uuid::parse("AC12F955-A29F-437B-A4C9-06C85B7FF57C"), -9000.0);
        item(&mut cache, RENDER_MESH_ID, 8000.0);
        cache.push(0);
        let extrusion = cat(&[&w.object(Uuid::parse("4ED7D4DB-E947-11d3-BFE5-0010830122F0"), b"curve", &[]), &[0; 155], &i32b(1), &[1, 1], &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cache)]);
        let extrusion = w.object(ON_EXTRUSION, &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 3, &extrusion), &[]);
        let chair = w.object(ON_MESH, &triangle(0.0), &[]);
        let mut xform = DMat4::from_translation(DVec3::new(5000.0, 0.0, 0.0)).transpose().to_cols_array().to_vec();
        xform.extend([0.0; 6]);
        let instance = w.object(ON_INSTANCE_REF, &cat(&[&[0x10], &id(40).0, &f64s(&xform)]), &[]);
        let points = [[0.0, 0.0, -1000.0], [1000.0, 0.0, -1000.0], [0.0, 1000.0, -1000.0]];
        let cloud = cat(&[&[0x11], &i32b(3), &f64s(&points.concat()), &[0; 180], &i32b(0), &i32b(3), &[255, 0, 0, 0, 0, 255, 0, 0, 0, 0, 255, 0]]);
        let cloud = w.object(ON_POINT_CLOUD, &cloud, &[]);
        let curve = w.object(Uuid::parse("4ED7D4DD-E947-11d3-BFE5-0010830122F0"), b"nurbs", &[]);
        let subd = w.object(ON_SUBD, b"subd", &[]);
        let objects = w.table(TCODE_OBJECT_TABLE, &[
            w.record(0x20, &slab, &attributes(id(30), 0, &[(1, wstr("Slab"))]), &[("Fire rating", "60 min"), ("Name", "user name")]),
            w.record(0x10, &brep, &attributes(id(31), 1, &[]), &[]),
            w.record(0x4000_0000, &extrusion, &attributes(id(32), 0, &[(6, vec![10, 20, 30, 0]), (13, vec![1])]), &[]),
            w.record(0x20, &chair, &attributes(id(41), 0, &[(12, vec![3]), (13, vec![3])]), &[]),
            w.record(0x1000, &instance, &attributes(id(33), 0, &[(1, wstr("Chair 1")), (6, vec![0, 255, 0, 0]), (13, vec![1])]), &[("Supplier", "Acme")]),
            w.record(0x20, &w.object(ON_MESH, &triangle(0.0), &[]), &attributes(id(34), 3, &[]), &[]),
            w.record(0x20, &w.object(ON_MESH, &triangle(0.0), &[]), &attributes(id(35), 0, &[(11, vec![0])]), &[]),
            w.record(0x2, &cloud, &attributes(id(36), 0, &[]), &[]),
            w.record(4, &curve, &attributes(id(37), 0, &[]), &[]),
            w.record(0x40000, &subd, &attributes(id(38), 0, &[]), &[]),
            w.record(0x20, &w.object(ON_MESH, &triangle(0.0), &[]), &attributes(id(39), 0, &[(19, vec![2])]), &[]),
        ]);
        w.file(70, 2_400_000_000, 2, &[materials, layers, blocks, objects])
    }

    #[test]
    fn rhino7_layers_objects_blocks_and_units() {
        let (model, info) = load("house.3DM", rhino7());
        assert_eq!(model.format, "Rhino");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Walls", "Walls::Glass", "Hidden::Off parent child", "Walls (points)"]);
        let walls = &model.layers[0];
        assert_eq!(walls.color, [200.0 / 255.0, 0.0, 0.0]);
        assert!(!walls.hidden);
        // the slab (quad, 2 triangles), the extrusion's render mesh and the block instance
        assert_eq!(walls.triangle_count(), 4);
        // millimetres to metres
        let slab: Vec<&[f32]> = walls.positions.chunks_exact(3).take(4).collect();
        assert_eq!(slab[2], [1.0, 2.0, 0.0]);
        assert!(walls.normals.chunks_exact(3).take(4).all(|n| n == [0.0, 0.0, 1.0]));
        // the extrusion's render mesh at 8 m, not the analysis mesh at -9 m
        assert!(walls.positions.chunks_exact(3).any(|p| p[2] == 8.0));
        assert!(walls.positions.chunks_exact(3).all(|p| p[2] > -5.0));
        let colors = walls.colors.as_ref().expect("object colours");
        assert_eq!(colors[0], [200, 0, 0, 255], "slab by layer");
        assert!(colors.contains(&[10, 20, 30, 255]), "extrusion by object");
        // the block definition's triangle, placed 5 m along x, coloured by its parent (green)
        let block: Vec<usize> = (0..walls.vertex_count()).filter(|&k| walls.positions[3 * k] >= 5.0).collect();
        assert_eq!(block.len(), 3);
        assert!(block.iter().all(|&k| colors[k] == [0, 255, 0, 255]));
        // the glass layer: child of Walls, coloured by its material, half transparent, but for
        // the first face of the polysurface, which has a colour of its own
        let glass = &model.layers[1];
        assert_eq!(glass.triangle_count(), 2);
        let glass_colors = glass.colors.as_ref().unwrap();
        assert_eq!(glass_colors[..3], [[255, 0, 0, 255]; 3]);
        assert_eq!(glass_colors[3..], [[0, 0, 255, 128]; 3]);
        assert!(glass.positions.chunks_exact(3).take(3).all(|p| p[2] == 3.0));
        assert!(glass.positions.chunks_exact(3).skip(3).all(|p| p[2] == 3.5));
        // a layer under a hidden parent starts hidden
        assert!(model.layers[2].hidden);
        // the point cloud in its own colours
        let points = &model.layers[3];
        assert!(points.points);
        assert_eq!(points.point_count(), 3);
        assert_eq!(points.colors.as_ref().unwrap()[1], [0, 255, 0, 255]);
        assert_eq!(points.positions[2], -1.0);
        // elements: slab, polysurface, extrusion, block instance, hidden-layer mesh
        assert_eq!(model.props.len(), 5);
        let slab = model.props.values().find(|p| p.contains("\"Slab\"")).expect("slab props");
        let slab: serde_json::Value = serde_json::from_str(slab).unwrap();
        assert_eq!(slab["Type"], "Mesh");
        assert_eq!(slab["Layer"], "Walls");
        assert_eq!(slab["Fire rating"], "60 min");
        assert_eq!(slab["Name (user text)"], "user name");
        assert_eq!(slab["Id"], id(30).to_string());
        let chair = model.props.values().find(|p| p.contains("Block instance")).expect("instance props");
        assert!(chair.contains("\"Name\":\"Chair 1\"") && chair.contains("\"Block\":\"Chair\"") && chair.contains("Acme"), "{chair}");
        assert!(model.props.values().any(|p| p.contains("\"Type\":\"Polysurface\"") && p.contains("Blue glass")));
        assert!(model.props.values().any(|p| p.contains("\"Type\":\"Extrusion\"")));
        assert_eq!(entry(&info, "Rhino file version"), "Rhino 7 (archive version 70)");
        assert_eq!(entry(&info, "Rhino application"), "Rhinoceros 7");
        assert_eq!(entry(&info, "Rhino units"), "millimetres (converted to metres)");
        assert_eq!(entry(&info, "Rhino objects drawn"), "1 block instances, 1 extrusions, 2 meshes, 1 point clouds, 1 polysurfaces");
        assert_eq!(entry(&info, "Rhino objects left out"), "1 SubD objects that could not be read, 1 curves, 1 hidden objects, 1 objects on layouts");
    }

    /// A Rhino 4 file: 4-byte chunk lengths, the attributes layout before Rhino 5, an
    /// uncompressed (1.x) mesh, a Rhino 4 material, a Rhino 5 extrusion with its meshes in user
    /// data, metres.
    #[test]
    fn rhino4_layout() {
        let w = W { big: false };
        let layers = w.table(TCODE_LAYER_TABLE, &[w.layer(0, "Default", [0, 0, 0, 0], true, id(1), Uuid::default(), 0)]);
        let material = w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&id(9).0, &i32b(0), &wstr("Oak"), &[0; 16], &[0; 4], &[150, 100, 50, 0], &[0; 16], &f64s(&[1.0, 0.0, 0.0, 0.0])]));
        let materials = w.table(TCODE_MATERIAL_TABLE, &[w.chunk(TCODE_MATERIAL_RECORD, &w.object(ON_MATERIAL, &cat(&[&[0x20], &material]), &[]))]);
        // a 1.x mesh: arrays of points, normals, texture coordinates, curvatures and colours
        let mut mesh = vec![0x10];
        mesh.extend(i32b(4));
        mesh.extend(i32b(1));
        mesh.extend(vec![0; 153]);
        mesh.extend(i32b(2));
        mesh.extend([0u16, 1, 2, 3].iter().flat_map(|i| i.to_le_bytes()));
        mesh.extend(i32b(4));
        mesh.extend(QUAD.iter().flatten().flat_map(|f| (f / 1000.0).to_le_bytes()));
        for _ in 0..4 {
            mesh.extend(i32b(0));
        }
        let old_attributes = |id: Uuid, name: &str| cat(&[&[0x12], &id.0, &i32b(0), &i32b(-1), &[0; 4], &[0; 20], &i32b(1), &[0, 0, 0, 0], &wstr(name), &wstr(""), &i32b(0), &[1]]);
        // version 1.6 (Rhino 4): display material references, plot fields, line type, then the
        // space, 1 for a layout
        let attributes_1_6 = |id: Uuid, space: u8| {
            let mut a = old_attributes(id, "");
            a[0] = 0x16;
            a.extend(cat(&[&i32b(0), &i32b(0), &[0], &[0; 4], &[0], &f64s(&[0.0]), &i32b(-1), &[space]]));
            a
        };
        let triangle = w.mesh(&[[0.0, 0.0, 5.0], [1.0, 0.0, 5.0], [0.0, 1.0, 5.0]], &[[0, 1, 2, 2]], None, None, true);
        let extrusion = cat(&[&w.object(Uuid::parse("4ED7D4DB-E947-11d3-BFE5-0010830122F0"), b"curve", &[]), &[0; 155], &i32b(1), &[1, 1]]);
        let cache = cat(&[&w.object(ON_MESH, &triangle, &[]), &w.object(Uuid::default(), &[], &[]), &w.object(Uuid::default(), &[], &[])]);
        let extrusion = w.object(ON_EXTRUSION, &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 2, &extrusion), &[(ON_V5_EXTRUSION_MESH_CACHE, cache)]);
        let objects = w.table(TCODE_OBJECT_TABLE, &[
            w.record(0x20, &w.object(ON_MESH, &mesh, &[]), &old_attributes(id(2), "Floor"), &[]),
            w.record(0x4000_0000, &extrusion, &old_attributes(id(3), ""), &[]),
            w.record(0x20, &w.object(ON_MESH, &triangle, &[]), &attributes_1_6(id(4), 0), &[]),
            // a logo on a layout
            w.record(0x20, &w.object(ON_MESH, &w.mesh(&[[0.0, 0.0, 90.0], [1.0, 0.0, 90.0], [0.0, 1.0, 90.0]], &[[0, 1, 2, 2]], None, None, true), &[]), &attributes_1_6(id(5), 1), &[]),
        ]);
        let (model, info) = load("old.3dm", w.file(4, 200_809_010, 4, &[materials, layers, objects]));
        assert_eq!(model.layers.len(), 1);
        let layer = &model.layers[0];
        assert_eq!(layer.name, "Default");
        assert_eq!(layer.triangle_count(), 4);
        // the layer's material colours everything
        assert_eq!(layer.colors.as_ref().unwrap()[0], [150, 100, 50, 255]);
        assert_eq!(model.bounds.min, [0.0, 0.0, 0.0]);
        assert_eq!(model.bounds.max, [1.0, 2.0, 5.0]);
        assert!(model.props.values().any(|p| p.contains("\"Name\":\"Floor\"") && p.contains("Oak")));
        assert_eq!(entry(&info, "Rhino file version"), "Rhino 4 (archive version 4)");
        assert_eq!(entry(&info, "Rhino units"), "metres");
        assert_eq!(entry(&info, "Rhino objects left out"), "1 objects on layouts");
    }

    #[test]
    fn black_layers_draw_grey_and_damaged_records_are_counted() {
        let w = W { big: true };
        let layers = w.table(TCODE_LAYER_TABLE, &[w.layer(0, "Default", [0, 0, 0, 0], true, id(1), Uuid::default(), -1)]);
        let good = w.record(0x20, &w.object(ON_MESH, &w.mesh(&QUAD, &[[0, 1, 2, 3]], None, None, false), &[]), &attributes(id(2), 0, &[]), &[]);
        // a mesh that claims 5 vertices but stores 4
        let mut bad = w.mesh(&QUAD, &[[0, 1, 2, 3]], None, None, false);
        bad[1] = 5;
        let bad = w.record(0x20, &w.object(ON_MESH, &bad, &[]), &attributes(id(3), 0, &[]), &[]);
        let (model, info) = load("grey.3dm", w.file(80, 2_400_000_000, 4, &[layers, w.table(TCODE_OBJECT_TABLE, &[good, bad])]));
        assert_eq!(model.layers[0].color, DEFAULT_COLOR.map(|v| (v * 255.0).round() / 255.0));
        assert!(model.layers[0].colors.is_none());
        assert_eq!(model.triangle_count(), 2);
        assert_eq!(entry(&info, "Rhino file version"), "Rhino 8 (archive version 80)");
        assert_eq!(entry(&info, "Rhino objects left out"), "1 unreadable objects");
    }

    #[test]
    fn files_without_meshes_say_why() {
        let w = W { big: true };
        let brep = w.object(BREP_ALIASES[0], &w.brep(&[]), &[]);
        let curve = w.object(Uuid::parse("4ED7D4E6-E947-11d3-BFE5-0010830122F0"), b"polyline", &[]);
        let objects = w.table(TCODE_OBJECT_TABLE, &[w.record(0x10, &brep, &attributes(id(1), 0, &[]), &[]), w.record(4, &curve, &attributes(id(2), 0, &[]), &[])]);
        let source = Source::Bytes { name: "small.3dm".into(), data: Arc::new(w.file(70, 2_400_000_000, 2, &[objects])) };
        let error = RhinoLoader.load(&source).unwrap_err();
        assert_eq!(error, "small.3dm: nothing to draw in this Rhino file (1 curves, 1 polysurfaces without a render mesh)");
    }

    #[test]
    fn uuids_round_trip_and_other_files_are_rejected() {
        assert_eq!(ON_MESH.to_string(), "4ed7d4e4-e947-11d3-bfe5-0010830122f0");
        assert_eq!(&ON_MESH.0[..4], &[0xE4, 0xD4, 0xD7, 0x4E]);
        let source = Source::Bytes { name: "x.3dm".into(), data: Arc::new(b"hello".to_vec()) };
        assert!(RhinoLoader.load(&source).unwrap_err().contains("Rhino"));
        // a header and then a truncated chunk
        let mut data = b"3D Geometry File Format       70".to_vec();
        data.extend([1, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0]);
        let source = Source::Bytes { name: "x.3dm".into(), data: Arc::new(data) };
        assert!(RhinoLoader.load(&source).is_err());
    }

    /// A block definition record (Rhino 6 layout) with the ids of its objects.
    fn block(w: &W, id: Uuid, name: &str, objects: &[Uuid]) -> Vec<u8> {
        let model_attributes = w.versioned(TCODE_MODEL_ATTRIBUTES_CHUNK, 1, 0, &cat(&[&[0], &[1], &id.0, &[0], &[0], &[1], &wstr(name)]));
        let units = w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&i32b(2), &f64s(&[0.001]), &wstr("")]));
        let list = cat(&[&i32b(objects.len() as i32), &objects.iter().flat_map(|o| o.0).collect::<Vec<u8>>()]);
        let definition = cat(&[&model_attributes, &i32b(1), &units, &wstr(""), &wstr(""), &wstr(""), &f64s(&[0.0; 6]), &[1], &list, &[0]]);
        w.chunk(TCODE_INSTANCE_DEFINITION_RECORD, &w.object(ON_INSTANCE_DEFINITION, &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &definition), &[]))
    }

    /// A block instance of `definition`, moved by `by`.
    fn instance(w: &W, definition: Uuid, by: DVec3) -> Vec<u8> {
        let mut xform = DMat4::from_translation(by).transpose().to_cols_array().to_vec();
        xform.extend([0.0; 6]);
        w.object(ON_INSTANCE_REF, &cat(&[&[0x10], &definition.0, &f64s(&xform)]), &[])
    }

    fn triangle(w: &W) -> Vec<u8> {
        w.object(ON_MESH, &w.mesh(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], &[[0, 1, 2, 2]], None, None, false), &[])
    }

    /// Block definition geometry: mode 3.
    const DEFINITION: (u8, [u8; 1]) = (12, [3]);

    #[test]
    fn blocks_that_contain_themselves_stop() {
        // block A holds a triangle and, twice, an instance of itself: without a check every level
        // doubles, 2^32 instances before the depth limit
        let w = W { big: true };
        let layers = w.table(TCODE_LAYER_TABLE, &[w.layer(0, "Default", [0, 0, 0, 0], true, id(1), Uuid::default(), -1)]);
        let blocks = w.table(TCODE_INSTANCE_DEFINITION_TABLE, &[block(&w, id(50), "A", &[id(60), id(60), id(61)])]);
        let definition = vec![(DEFINITION.0, DEFINITION.1.to_vec())];
        let objects = w.table(TCODE_OBJECT_TABLE, &[
            w.record(0x1000, &instance(&w, id(50), DVec3::X), &attributes(id(60), 0, &definition), &[]),
            w.record(0x20, &triangle(&w), &attributes(id(61), 0, &definition), &[]),
            w.record(0x1000, &instance(&w, id(50), DVec3::ZERO), &attributes(id(62), 0, &[]), &[]),
        ]);
        let (model, info) = load("cycle.3dm", w.file(70, 2_400_000_000, 4, &[layers, blocks, objects]));
        assert_eq!(model.triangle_count(), 1);
        assert_eq!(entry(&info, "Rhino objects left out"), "2 blocks that contain themselves");
    }

    #[test]
    fn wide_nested_blocks_stop_at_the_part_limit() {
        // 20 levels of blocks, each with its next level 4 times: 4^20 instances, but no cycle
        let w = W { big: true };
        let layers = w.table(TCODE_LAYER_TABLE, &[w.layer(0, "Default", [0, 0, 0, 0], true, id(1), Uuid::default(), -1)]);
        let level = |k: u8| Uuid([k, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
        let member = |k: u8| Uuid([k, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9]);
        let blocks: Vec<Vec<u8>> = (0..20).map(|k| block(&w, level(k), "level", &[member(k + 1); 4])).collect();
        let definition = vec![(DEFINITION.0, DEFINITION.1.to_vec())];
        let mut objects: Vec<Vec<u8>> = (1..20).map(|k| w.record(0x1000, &instance(&w, level(k), DVec3::ZERO), &attributes(member(k), 0, &definition), &[])).collect();
        objects.push(w.record(0x1000, &instance(&w, level(0), DVec3::ZERO), &attributes(id(70), 0, &[]), &[]));
        objects.push(w.record(0x20, &triangle(&w), &attributes(id(71), 0, &[]), &[]));
        let file = w.file(70, 2_400_000_000, 4, &[layers, w.table(TCODE_INSTANCE_DEFINITION_TABLE, &blocks), w.table(TCODE_OBJECT_TABLE, &objects)]);
        let (model, info) = load("wide.3dm", file);
        assert_eq!(model.triangle_count(), 1);
        assert!(entry(&info, "Rhino objects left out").contains("1 block instances cut short (too many parts)"), "{info:?}");
    }

    #[test]
    fn layer_parent_cycles_end() {
        let w = W { big: true };
        let layers = w.table(TCODE_LAYER_TABLE, &[
            w.layer(0, "A", [0, 0, 0, 0], true, id(1), id(2), -1),
            w.layer(1, "B", [0, 0, 0, 0], false, id(2), id(1), -1),
            w.layer(2, "C", [0, 0, 0, 0], true, id(3), id(3), -1),
        ]);
        let objects = w.table(TCODE_OBJECT_TABLE, &[w.record(0x20, &triangle(&w), &attributes(id(10), 0, &[]), &[]), w.record(0x20, &triangle(&w), &attributes(id(11), 2, &[]), &[])]);
        let (model, _) = load("cycle.3dm", w.file(70, 2_400_000_000, 4, &[layers, objects]));
        let names: Vec<(&str, bool)> = model.layers.iter().map(|l| (l.name.as_str(), l.hidden)).collect();
        assert_eq!(names, [("B::A", true), ("C", false)]);
    }

    #[test]
    fn block_instances_take_layer_visibility_and_paint_from_parent() {
        let w = W { big: true };
        let layers = w.table(TCODE_LAYER_TABLE, &[
            w.layer(0, "Default", [0, 0, 0, 0], true, id(1), Uuid::default(), -1),
            w.layer(1, "Furniture", [0, 0, 200, 0], false, id(2), Uuid::default(), -1),
            w.layer(2, "Green", [0, 200, 0, 0], true, id(3), Uuid::default(), -1),
            w.layer(3, "Glass", [100, 100, 100, 0], true, id(4), Uuid::default(), 0),
        ]);
        let glass = cat(&[&w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&0x0Cu32.to_le_bytes(), &i32b(0), &wstr("Glass")])), &[0; 16], &[0; 4], &[0, 0, 255, 0], &[0; 16], &f64s(&[1.0, 0.0, 0.0, 0.5])]);
        let materials = w.table(TCODE_MATERIAL_TABLE, &[w.chunk(TCODE_MATERIAL_RECORD, &w.object(ON_MATERIAL, &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &glass), &[]))]);
        let blocks = w.table(TCODE_INSTANCE_DEFINITION_TABLE, &[block(&w, id(50), "Chair", &[id(60)])]);
        // the chair: red by object, material by parent, on the default layer
        let red = [(12, vec![3]), (6, vec![255, 0, 0, 0]), (13, vec![1]), (16, vec![3])];
        let objects = w.table(TCODE_OBJECT_TABLE, &[
            w.record(0x20, &triangle(&w), &attributes(id(60), 0, &red), &[]),
            // on a layer that is off: the chair goes to that (hidden) layer
            w.record(0x1000, &instance(&w, id(50), DVec3::ZERO), &attributes(id(70), 1, &[]), &[]),
            // on a green layer without a material: the chair stays red
            w.record(0x1000, &instance(&w, id(50), DVec3::Z), &attributes(id(71), 2, &[]), &[]),
            // material by parent outside a block: the layer's material
            w.record(0x20, &triangle(&w), &attributes(id(72), 3, &[(16, vec![3])]), &[]),
        ]);
        let (model, _) = load("chairs.3dm", w.file(70, 2_400_000_000, 4, &[materials, layers, blocks, objects]));
        let names: Vec<(&str, bool, usize)> = model.layers.iter().map(|l| (l.name.as_str(), l.hidden, l.triangle_count())).collect();
        assert_eq!(names, [("Default", false, 1), ("Furniture", true, 1), ("Glass", false, 1)]);
        assert_eq!(model.layers[0].colors.as_ref().unwrap(), &[[255, 0, 0, 255]; 3]);
        assert_eq!(model.layers[2].colors.as_ref().unwrap(), &[[0, 0, 255, 128]; 3]);
    }

    #[test]
    fn survey_coordinates_move_to_a_local_origin() {
        // a block placed and a point cloud at Dutch RD coordinates, in metres
        let w = W { big: true };
        let layers = w.table(TCODE_LAYER_TABLE, &[w.layer(0, "Default", [0, 0, 0, 0], true, id(1), Uuid::default(), -1)]);
        let blocks = w.table(TCODE_INSTANCE_DEFINITION_TABLE, &[block(&w, id(50), "Tree", &[id(60)])]);
        let points = [155_000.125, 463_000.0625, 1.0, 155_010.5, 463_020.25, 2.0];
        let cloud = w.object(ON_POINT_CLOUD, &cat(&[&[0x10], &i32b(2), &f64s(&points), &[0; 180]]), &[]);
        let objects = w.table(TCODE_OBJECT_TABLE, &[
            w.record(0x20, &triangle(&w), &attributes(id(60), 0, &[(DEFINITION.0, DEFINITION.1.to_vec())]), &[]),
            w.record(0x1000, &instance(&w, id(50), DVec3::new(155_000.25, 463_000.5, 0.0)), &attributes(id(70), 0, &[]), &[]),
            w.record(0x2, &cloud, &attributes(id(71), 0, &[]), &[]),
        ]);
        let (model, info) = load("rd.3dm", w.file(70, 2_400_000_000, 4, &[layers, blocks, objects]));
        assert_eq!(entry(&info, "Rhino world origin (metres)"), "155000, 463000, 0");
        let mesh = &model.layers[0];
        assert_eq!(mesh.positions[..6], [0.25, 0.5, 0.0, 1.25, 0.5, 0.0]);
        let cloud = &model.layers[1];
        assert_eq!(cloud.positions, [0.125, 0.0625, 1.0, 10.5, 20.25, 2.0]);
    }

    /// Models made in Rhino 9 and saved without render meshes (`test-models/rhino`, from a script
    /// as Rhino writes files it never drew): every polysurface and extrusion is meshed by the
    /// viewer, with sound triangles.
    #[test]
    fn polysurfaces_and_extrusions_without_meshes_are_meshed() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/rhino");
        // objects meshed: a block definition's members once, however often it is placed
        for (name, objects) in [("primitives", 7), ("booleans", 3), ("extrusions", 5), ("freeform", 5), ("building", 12), ("tower", 10), ("mechanical", 3), ("stairs", 22), ("blocks", 8), ("dome", 2), ("roofs", 3)] {
            let data = std::fs::read(dir.join(format!("{name}.3dm"))).unwrap();
            let (model, info) = load(&format!("{name}.3dm"), data);
            assert!(info.iter().all(|(k, _)| k != "Rhino objects left out"), "{name}: {info:?}");
            assert_eq!(entry(&info, "Rhino objects meshed by the viewer"), format!("{objects} (saved without a render mesh)"), "{name}");
            let mut triangles = 0;
            for layer in &model.layers {
                assert!(layer.positions.iter().chain(&layer.normals).all(|v| v.is_finite()), "{name}");
                assert!(layer.normals.chunks_exact(3).all(|n| (n[0] * n[0] + n[1] * n[1] + n[2] * n[2] - 1.0).abs() < 1e-3), "{name}: unit normals");
                triangles += layer.indices.len() / 3;
            }
            assert!(triangles > 50 * objects, "{name}: {triangles} triangles");
        }
    }

    /// The area of every object meshed by the viewer is the area Rhino computes for its exact
    /// geometry (`AreaMassProperties`), to within the chordal error of the mesh: trims, holes
    /// and caps are all there, and nothing is meshed twice.
    #[test]
    fn meshed_areas_match_rhino() {
        // file, object name, area in model units (square millimetres) as Rhino 9 computes it
        let expected = [
        ("primitives", "Sphere", 314.1593),
        ("primitives", "Cylinder", 351.8584),
        ("primitives", "Cone", 185.6094),
        ("primitives", "Torus", 394.7842),
        ("primitives", "Box", 448.0000),
        ("primitives", "Plane", 560.0000),
        ("primitives", "Wavy patch", 329.3163),
        ("booleans", "Plate with holes", 999.0885),
        ("booleans", "Box and tower", 510.7964),
        ("booleans", "Sphere with a tunnel", 403.0810),
        ("extrusions", "Column", 89.5354),
        ("extrusions", "Tube", 257.9200),
        ("extrusions", "I-beam", 125.0400),
        ("extrusions", "Wave wall", 34.4615),
        ("extrusions", "Beam", 81.2000),
        ("freeform", "Vase", 139.9704),
        ("freeform", "Revolved", 120.2451),
        ("freeform", "Sweep", 59.8317),
        ("freeform", "Patch with a hole", 127.7938),
        ("freeform", "Edge surface", 44.4351),
        ("building", "Window 1", 5.0480),
        ("building", "Window 2", 5.0480),
        ("building", "Window 3", 5.0480),
        ("building", "Window 4", 5.0480),
        ("building", "Walls", 317.0467),
        ("building", "Ground slab", 390.8000),
        ("building", "Roof", 440.9390),
        ("building", "Column 1", 4.0212),
        ("building", "Column 2", 4.0212),
        ("building", "Column 3", 4.0212),
        ("building", "Column 4", 4.0212),
        ("building", "Column 5", 4.0212),
        ("tower", "Twisted tower", 2152.8922),
        ("tower", "Balcony 1", 299.8000),
        ("tower", "Balcony 2", 299.8000),
        ("tower", "Balcony 3", 299.8000),
        ("tower", "Balcony 4", 299.8000),
        ("tower", "Balcony 5", 299.8000),
        ("tower", "Balcony 6", 299.8000),
        ("tower", "Balcony 7", 299.8000),
        ("tower", "Ring slab", 202.9469),
        ("tower", "Canopy", 175.5441),
        ("mechanical", "Flange", 855.8850),
        ("mechanical", "Gear", 316.6037),
        ("mechanical", "Bracket", 220.2145),
        ("stairs", "Column", 11.8752),
        ("stairs", "Step 1", 3.5360),
        ("stairs", "Step 2", 3.5360),
        ("stairs", "Step 3", 3.5360),
        ("stairs", "Step 4", 3.5360),
        ("stairs", "Step 5", 3.5360),
        ("stairs", "Step 6", 3.5360),
        ("stairs", "Step 7", 3.5360),
        ("stairs", "Step 8", 3.5360),
        ("stairs", "Step 9", 3.5360),
        ("stairs", "Step 10", 3.5360),
        ("stairs", "Step 11", 3.5360),
        ("stairs", "Step 12", 3.5360),
        ("stairs", "Step 13", 3.5360),
        ("stairs", "Step 14", 3.5360),
        ("stairs", "Step 15", 3.5360),
        ("stairs", "Step 16", 3.5360),
        ("stairs", "Step 17", 3.5360),
        ("stairs", "Step 18", 3.5360),
        ("stairs", "Step 19", 3.5360),
        ("stairs", "Step 20", 3.5360),
        ("stairs", "Handrail", 3.6224),
        ("dome", "Dome", 408.1008),
        ("dome", "Drum", 320.6938),
        ("roofs", "Hypar", 125.2610),
        ("roofs", "SubD blob", 106.3228),
        ("roofs", "Ribbon", 30.7790),
        // three table sets of four chairs (nested blocks) and a mirrored chair
        ("blocks", "", 26.1531),
        ];
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/rhino");
        let mut areas: HashMap<(String, String), f64> = HashMap::new();
        for name in ["primitives", "booleans", "extrusions", "freeform", "building", "tower", "mechanical", "stairs", "dome", "roofs", "blocks"] {
            let (model, _) = load(&format!("{name}.3dm"), std::fs::read(dir.join(format!("{name}.3dm"))).unwrap());
            for layer in &model.layers {
                let p = |i: u32| DVec3::from_slice(&layer.positions[3 * i as usize..3 * i as usize + 3].iter().map(|&v| v as f64).collect::<Vec<_>>());
                for &(id, start, count) in &layer.elements {
                    let props: serde_json::Value = serde_json::from_str(&model.props[&id]).unwrap();
                    let object = props["Name"].as_str().unwrap_or_default().to_string();
                    let tris = &layer.indices[start as usize..(start + count) as usize];
                    let area: f64 = tris.chunks_exact(3).map(|t| 0.5 * (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length()).sum();
                    *areas.entry((name.to_string(), object)).or_default() += area;
                }
            }
        }
        let mut wrong = Vec::new();
        for (file, object, rhino) in expected {
            // the models are in millimetres, the viewer in metres
            // an empty name: the whole file
            let ours = 1e6 * areas.iter().filter(|((f, o), _)| f == file && (object.is_empty() || o == object)).map(|(_, a)| a).sum::<f64>();
            if (ours - rhino).abs() > 0.015 * rhino {
                wrong.push(format!("{file} {object}: {ours:.3} against {rhino} in Rhino"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("
"));
    }

    /// A Rhino 9 file with what a model usually holds, saved as Rhino saves it (with render
    /// meshes) and without them: layers and sublayers, a hidden and a locked layer, colours by
    /// layer, object and parent, a basic material with transparency, a PBR material, a layer
    /// material, colours per face, user text, an extrusion, a sphere, a mesh with vertex colours,
    /// a block, points, a SubD, a curve, a text, a hatch, and a hidden object it leaves out. Both
    /// read the same.
    #[test]
    fn rhino9_layers_materials_colours_and_user_text() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/rhino");
        for (file, meshed) in [("everything_meshes.3dm", 2), ("everything_small.3dm", 14)] {
            let (model, info) = load(file, std::fs::read(dir.join(file)).unwrap());
            assert_eq!(entry(&info, "Rhino file version"), "Rhino 9 (archive version 90)");
            assert_eq!(entry(&info, "Rhino objects drawn"), "1 SubD objects, 1 block instances, 1 curves, 1 extrusions, 1 hatches, 1 meshes, 1 point clouds, 1 points, 10 polysurfaces, 1 surfaces, 1 texts", "{file}");
            assert_eq!(entry(&info, "Rhino objects left out"), "1 hidden objects", "{file}");
            // the SubD, and the box on the hidden layer which Rhino left without a mesh, are meshed
            // by the viewer
            assert_eq!(entry(&info, "Rhino objects meshed by the viewer"), format!("{meshed} (saved without a render mesh)"), "{file}");
            let layer = |name: &str| model.layers.iter().find(|l| l.name == name).unwrap_or_else(|| panic!("{file}: no layer {name}"));
            assert_eq!(layer("Walls::Interior").color, [40.0 / 255.0, 40.0 / 255.0, 200.0 / 255.0]);
            assert!(layer("Hidden layer").hidden && !layer("Locked layer").hidden);
            assert_eq!(layer("Misc (points)").point_count(), 101);
            // element name -> its colours and properties
            let mut elements: HashMap<String, (Vec<[u8; 4]>, serde_json::Value)> = HashMap::new();
            for l in &model.layers {
                for &(id, start, count) in &l.elements {
                    let props: serde_json::Value = serde_json::from_str(&model.props[&id]).unwrap();
                    let mut colours: Vec<[u8; 4]> = l.indices[start as usize..(start + count) as usize].iter().filter_map(|&i| l.colors.as_ref().map(|c| c[i as usize])).collect();
                    colours.sort();
                    colours.dedup();
                    elements.insert(props["Name"].as_str().unwrap_or_default().to_string(), (colours, props));
                }
            }
            let colours = |name: &str| elements.get(name).unwrap_or_else(|| panic!("{file}: no {name}")).0.clone();
            assert_eq!(colours("Box object colour"), [[250, 200, 0, 255]]);
            assert_eq!(colours("Box basic material"), [[220, 30, 30, 128]], "half transparent");
            assert_eq!(colours("Box PBR material"), [[30, 80, 220, 255]]);
            assert_eq!(colours("Box layer material"), [[40, 180, 60, 255]]);
            assert_eq!(colours("Box face colours").len(), 6);
            assert_eq!(colours("Crate by parent"), [[0, 200, 200, 255]]);
            assert!(colours("Coloured mesh").len() > 100);
            let props = &elements["Box user text"].1;
            assert_eq!((props["Fire rating"].as_str(), props["Supplier"].as_str()), (Some("60 min"), Some("ACME")));
            assert!(!elements.contains_key("Hidden box"));
        }
    }

    /// SubD objects made in Rhino 9 (`subd.3dm`, from make_subd.py): smooth, with creased edges,
    /// an open patch with corners, triangles, a pentagon, semi-sharp edges of constant and of
    /// varying sharpness, a dart and a fine ring. Rhino keeps no mesh of them; the viewer's mesh
    /// of the limit surface has the area Rhino computes for its exact NURBS form, within 0.5%.
    #[test]
    fn subd_limit_surfaces_match_rhino() {
        // name, area in square millimetres of the limit surface (SubD.ToBrep) in Rhino 9
        let expected = [
            ("Smooth box", 20.3797),
            ("Creased top", 28.7320),
            ("Open patch", 20.0852),
            ("Triangle ball", 24.2780),
            ("Pentagon prism", 16.7190),
            ("Sharp box", 24.2653),
            ("Varying sharp box", 24.1884),
            ("Dart", 38.6362),
            ("Ring", 117.3877),
        ];
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/rhino/subd.3dm");
        let (model, info) = load("subd.3dm", std::fs::read(path).unwrap());
        assert_eq!(entry(&info, "Rhino objects drawn"), "9 SubD objects");
        let mut areas = HashMap::new();
        for layer in &model.layers {
            let p = |i: u32| DVec3::from_slice(&layer.positions[3 * i as usize..3 * i as usize + 3].iter().map(|&v| v as f64).collect::<Vec<_>>());
            for &(id, start, count) in &layer.elements {
                let props: serde_json::Value = serde_json::from_str(&model.props[&id]).unwrap();
                assert_eq!(props["Type"], "SubD");
                let tris = &layer.indices[start as usize..(start + count) as usize];
                let area: f64 = tris.chunks_exact(3).map(|t| 0.5 * (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length()).sum();
                areas.insert(props["Name"].as_str().unwrap().to_string(), 1e6 * area);
            }
        }
        for (name, rhino) in expected {
            let ours = areas[name];
            assert!((ours - rhino).abs() <= 0.005 * rhino, "{name}: {ours} against {rhino} in Rhino");
        }
    }

    /// Shapes that once meshed wrong, made in Rhino 9 (`regress.3dm`, from make_regress.py) after
    /// models of McNeel's openNURBS examples: a short conical band of a rational circle (thin
    /// slivers along an edge of almost collinear points cut across the circle), strips of arcs
    /// meeting at kinks (triangles over a kink zigzagged and added area), and a mesh of bent
    /// quads (split along the shorter diagonal, as Rhino does).
    #[test]
    fn slivers_kinks_and_bent_quads_keep_their_area() {
        // name, area in Rhino 9, allowed deviation
        let expected = [
            ("Lens band", 8.120637, 0.015),
            ("Thin ring", 2.513274, 0.015),
            ("Arc strip", 3.792911, 0.015),
            ("Arc band", 28.262329, 0.015),
            ("Twisted arc strip", 6.055392, 0.015),
            ("Bent quads", 37.749172, 0.0001),
        ];
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/rhino/regress.3dm");
        let (model, _) = load("regress.3dm", std::fs::read(path).unwrap());
        let mut areas = HashMap::new();
        for layer in &model.layers {
            let p = |i: u32| DVec3::from_slice(&layer.positions[3 * i as usize..3 * i as usize + 3].iter().map(|&v| v as f64).collect::<Vec<_>>());
            for &(id, start, count) in &layer.elements {
                let props: serde_json::Value = serde_json::from_str(&model.props[&id]).unwrap();
                let tris = &layer.indices[start as usize..(start + count) as usize];
                let area: f64 = tris.chunks_exact(3).map(|t| 0.5 * (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length()).sum();
                areas.insert(props["Name"].as_str().unwrap().to_string(), 1e6 * area);
            }
        }
        for (name, rhino, allowed) in expected {
            let ours = areas[name];
            assert!((ours - rhino).abs() <= allowed * rhino, "{name}: {ours} against {rhino} in Rhino");
        }
    }

    /// A closed polysurface meshes into a closed mesh: the faces on either side of an edge share
    /// its points (on Rhino's edge curve), so no crack opens between them. Every edge of the
    /// welded mesh of each solid is used by two triangles.
    #[test]
    fn closed_polysurfaces_mesh_watertight() {
        let solids = [
            ("primitives", vec!["Sphere", "Cylinder", "Cone", "Torus", "Box"]),
            ("booleans", vec!["Plate with holes", "Box and tower", "Sphere with a tunnel"]),
            ("extrusions", vec!["Column", "Tube", "I-beam", "Beam"]),
            ("mechanical", vec!["Flange", "Gear", "Bracket"]),
            ("fillets", vec![]),
        ];
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/rhino");
        for (file, names) in solids {
            let Ok(data) = std::fs::read(dir.join(format!("{file}.3dm"))) else { continue };
            let (model, _) = load(&format!("{file}.3dm"), data);
            for layer in &model.layers {
                for &(id, start, count) in &layer.elements {
                    let props: serde_json::Value = serde_json::from_str(&model.props[&id]).unwrap();
                    let name = props["Name"].as_str().unwrap_or_default();
                    if !names.contains(&name) {
                        continue;
                    }
                    // weld by exact position
                    let mut ids: HashMap<[u32; 3], u32> = HashMap::new();
                    let mut weld = |i: u32| {
                        let k = [0, 1, 2].map(|c| layer.positions[3 * i as usize + c].to_bits());
                        let n = ids.len() as u32;
                        *ids.entry(k).or_insert(n)
                    };
                    let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
                    for t in layer.indices[start as usize..(start + count) as usize].chunks_exact(3) {
                        let w = [weld(t[0]), weld(t[1]), weld(t[2])];
                        for k in 0..3 {
                            let (a, b) = (w[k], w[(k + 1) % 3]);
                            if a != b {
                                *edges.entry((a.min(b), a.max(b))).or_default() += 1;
                            }
                        }
                    }
                    let open = edges.values().filter(|&&n| n == 1).count();
                    assert_eq!(open, 0, "{file} {name}: {open} open edges of {}", edges.len());
                }
            }
        }
    }

    /// Curves, texts and hatches made in Rhino 9 (`drafting.3dm`, from make_drafting.py). Its
    /// dimension style draws texts twice their height in model space; each text covers the area
    /// Rhino's outlines of it cover (in the viewer's typeface, so a little narrower or wider).
    #[test]
    fn curves_texts_and_hatches_are_drawn() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/rhino/drafting.3dm");
        let (model, info) = load("drafting.3dm", std::fs::read(path).unwrap());
        assert_eq!(entry(&info, "Rhino objects drawn"), "7 curves, 5 hatches, 6 texts");
        let curves = model.layers.iter().find(|l| l.name == "Curves").unwrap();
        assert!(curves.lines.len() > 7 * 2 * 8, "curves are drawn as line segments");
        let hatches = model.layers.iter().find(|l| l.name == "Hatches").unwrap();
        assert!(!hatches.indices.is_empty() && !hatches.lines.is_empty(), "a solid fill and pattern lines");
        // Rhino's outlines of the texts: x and y from and to
        let rhino = [
            ("Text 1", [0.1097, -12.0170, 8.1377, -10.9966]),
            ("Text 2", [0.1858, -16.0426, 8.7031, -13.4591]),
            ("Text 3", [15.0327, -12.2938, 18.7485, -9.4007]),
            ("Text 4", [24.7322, -11.8019, 31.4700, -7.4428]),
            ("Text 5", [37.8264, -12.5164, 42.2054, -11.5007]),
            ("Text 6", [0.0545, -20.1977, 8.1241, -18.9836]),
        ];
        let texts = model.layers.iter().find(|l| l.name == "Text").unwrap();
        for (name, outline) in rhino {
            // the model is in millimetres, the viewer in metres
            let [x0, y0, x1, y1] = outline.map(|v| v / 1000.0);
            let mut low = [f64::MAX; 3];
            let mut high = [f64::MIN; 3];
            for &(id, start, count) in &texts.elements {
                let props: serde_json::Value = serde_json::from_str(&model.props[&id]).unwrap();
                if props["Name"].as_str() != Some(name) {
                    continue;
                }
                for &i in &texts.indices[start as usize..(start + count) as usize] {
                    for c in 0..3 {
                        let v = texts.positions[3 * i as usize + c] as f64;
                        low[c] = low[c].min(v);
                        high[c] = high[c].max(v);
                    }
                }
            }
            assert!(low[0] < high[0], "{name} is drawn");
            let (w, h) = (x1 - x0, y1 - y0);
            let (ours_w, ours_h) = (high[0] - low[0], high[1] - low[1]);
            assert!((ours_h - h).abs() < 0.15 * h, "{name}: {ours_h} high against {h} in Rhino");
            assert!((ours_w - w).abs() < 0.2 * w, "{name}: {ours_w} wide against {w} in Rhino");
            let (mid_x, mid_y) = ((low[0] + high[0]) / 2.0, (low[1] + high[1]) / 2.0);
            assert!((mid_x - (x0 + x1) / 2.0).abs() < 0.15 * w && (mid_y - (y0 + y1) / 2.0).abs() < 0.15 * h, "{name}: around ({mid_x}, {mid_y})");
        }
    }

    /// Dimensions and leaders made in Rhino 9 (`dimensions.3dm`, from make_dimensions.py; the
    /// reader checks their lines and texts against Rhino's): each is one element, drawn with
    /// lines and filled arrowheads and text.
    #[test]
    fn dimensions_and_leaders_are_drawn() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/rhino/dimensions.3dm");
        let (model, info) = load("dimensions.3dm", std::fs::read(path).unwrap());
        assert_eq!(entry(&info, "Rhino objects drawn"), "2 curves, 9 dimensions, 2 leaders");
        let dims = model.layers.iter().find(|l| l.name == "Dimensions").unwrap();
        let elements = |l: &Layer| l.elements.iter().map(|e| e.0).chain(l.line_elements.iter().map(|e| e.0)).collect::<HashSet<u64>>();
        // nine dimensions and two circles
        assert_eq!(elements(dims).len(), 11);
        assert!(!dims.indices.is_empty() && !dims.lines.is_empty(), "lines, and arrowheads and text");
        // the horizontal dimension's line, 10 mm long at 3 mm up, in metres
        let p = |i: u32| [0, 1, 2].map(|k| dims.positions[3 * i as usize + k] as f64);
        let found = dims.lines.chunks_exact(2).any(|s| {
            let (a, b) = (p(s[0]), p(s[1]));
            (a[1] - 0.003).abs() < 1e-9 && (b[1] - 0.003).abs() < 1e-9 && ((a[0] - b[0]).abs() - 0.010).abs() < 1e-9
        });
        assert!(found, "the dimension line of Horizontal");
    }
}
