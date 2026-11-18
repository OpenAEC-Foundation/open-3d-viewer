//! Rhino 3D models (`.3dm`), Rhino 1 to 9. The file is read by the `threedm-read` crate
//! ([rhino-dll](https://github.com/OpenAEC-Foundation/rhino-dll)): its layers, materials, block
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
//! cannot exhaust memory or run for ever. Lights are drawn as a small cross and the line they shine
//! along. Layouts become sheets, their details viewports. What is left out is
//! counted in the project information; hidden objects go to hidden layers of their own. Clipping planes are drawn; the one that clips the
//! perspective view is the section the model opens with.

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{DraftFill, Layer, ProjectData, SavedView, Sheet, SheetRect, SheetViewport, ViewCamera, ViewKind};
use crate::section::SectionPlane;
use glam::{DMat3, DMat4, DVec3};
use std::collections::{BTreeMap, HashMap, HashSet};
use threedm_read::{Attributes, ClippingPlane, File, Geometry, Material, Mesh, RhinoLayer, Uuid};

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
        let (layers, info, section, views, sheets) = convert(&file);
        if layers.iter().all(|l| l.indices.is_empty() && l.lines.is_empty() && !(l.points && !l.positions.is_empty())) {
            // say why, e.g. polysurfaces saved without their render meshes
            let left_out = info.iter().find(|(k, _)| k == "Rhino objects left out").map_or("no objects", |(_, v)| v.as_str());
            return Err(format!("{}: nothing to draw in this Rhino file ({left_out})", source.name()));
        }
        Ok(Loaded { structural: Vec::new(), origin: None, layers, views, project: ProjectData { info, section, sheets, ..ProjectData::default() } })
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
    /// The viewer's layers by Rhino layer, and whether they hold its hidden objects.
    meshes: BTreeMap<(Option<usize>, bool), Layer>,
    points: BTreeMap<(Option<usize>, bool), Layer>,
    /// Drawing hidden objects: they go to hidden layers of their own.
    hiding: bool,
    /// What the layouts hold, by the id of their page viewport.
    pages: HashMap<Uuid, PageContent>,
    /// Hidden objects drawn (on those layers).
    hidden_objects: usize,
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

/// What a layout holds: lines and filled shapes on the paper (metres) and its details.
#[derive(Default)]
struct PageContent {
    /// The objects on it.
    objects: usize,
    lines: Vec<[[f32; 2]; 2]>,
    fills: Vec<DraftFill>,
    details: Vec<PageDetail>,
}

/// A detail on a layout: its outline on the paper (metres), the model view it shows and its
/// scale (page length per model length, in their units).
struct PageDetail {
    name: String,
    boundary: Vec<[f32; 2]>,
    view: threedm_read::View,
    scale: Option<f64>,
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
    /// material (a physically based one's base colour and opacity, as Rhino shows it), else the
    /// display colour.
    fn rgba(&self) -> Rgba {
        match self.material {
            Some(Material { pbr: Some(p), .. }) => {
                let c = |v: f32| (255.0 * v.clamp(0.0, 1.0)).round() as u8;
                [c(p.base_color[0]), c(p.base_color[1]), c(p.base_color[2]), (255.0 * p.opacity.clamp(0.0, 1.0)).round() as u8]
            }
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
            hiding: false,
            pages: HashMap::new(),
            hidden_objects: 0,
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
        let key = (self.layers.get(&layer).copied(), self.hiding);
        if (if points { &self.points } else { &self.meshes }).contains_key(&key) {
            let map = if points { &mut self.points } else { &mut self.meshes };
            return map.get_mut(&key).unwrap();
        }
        let (name, color, hidden) = match key.0 {
            Some(k) => {
                let color = display(opaque(self.file.layers[k].color));
                let (path, shown) = self.lineage(k).clone();
                (path, color, !shown)
            }
            None => ("Default".to_string(), display([0, 0, 0, 255]), false),
        };
        let map = if points { &mut self.points } else { &mut self.meshes };
        let name = if key.1 { format!("{name} (hidden objects)") } else { name };
        map.entry(key).or_insert_with(|| Layer {
            name: if points { format!("{name} (points)") } else { name },
            color: [color[0] as f32 / 255.0, color[1] as f32 / 255.0, color[2] as f32 / 255.0],
            hidden: hidden || key.1,
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

    /// What a clipping plane clips: the views (by name), how deep and which objects.
    fn clip_props(&self, p: &ClippingPlane) -> Vec<(String, String)> {
        let views: Vec<&str> = self.file.views.iter().filter(|v| p.enabled && p.viewports.contains(&v.id)).map(|v| v.name.as_str()).collect();
        let mut out = vec![("Clips".to_string(), if views.is_empty() { "no view (off)".to_string() } else { views.join(", ") })];
        if let Some(d) = p.depth {
            out.push(("Clipping depth (m)".into(), format!("{}", d * self.file.meters)));
        }
        if let Some(s) = &p.selection {
            let what = format!("{} objects, {} layers", s.objects.len(), s.layers.len());
            out.push(("Clips only".into(), if s.except { format!("all but {what}") } else { what }));
        }
        out
    }

    /// The cut the file opens with: the first clipping plane in model space that clips a
    /// perspective view of the file, as a section plane in display coordinates. Clipping planes
    /// that clip to a depth or clip only some objects are not taken: the viewer's section plane
    /// cuts everything beyond it.
    fn section(&self) -> (Option<SectionPlane>, usize) {
        let perspective: Vec<Uuid> = self.file.views.iter().filter(|v| v.perspective).map(|v| v.id).collect();
        let planes: Vec<&ClippingPlane> = self
            .file
            .objects
            .iter()
            .filter(|o| o.attributes.mode != 3 && o.attributes.space != 2)
            .filter_map(|o| match &o.geometry {
                Geometry::ClippingPlane(p) if p.enabled && p.viewports.iter().any(|v| perspective.contains(v)) => Some(p),
                _ => None,
            })
            .collect();
        let origin = self.origin.unwrap_or(DVec3::ZERO);
        let section = planes.iter().find(|p| p.depth.is_none() && p.selection.is_none()).and_then(|p| {
            let normal = p.normal.normalize_or_zero();
            let at = p.origin * self.file.meters - origin;
            (normal != DVec3::ZERO && at.is_finite()).then(|| SectionPlane { normal: normal.as_vec3(), offset: normal.dot(at) as f32 })
        });
        (section, planes.len())
    }

    /// An object on a layout, kept for its sheet: its lines and filled shapes (texts, hatches,
    /// arrowheads) flat on the paper, or a detail: the model view it shows, where.
    fn page_object(&mut self, object: &threedm_read::RhinoObject<'_>) {
        let a = &object.attributes;
        let unit = self.file.page_meters as f32;
        let flat = |p: &DVec3| [p.x as f32 * unit, p.y as f32 * unit];
        let color = self.paint(a, None).rgba();
        let content = self.pages.entry(a.viewport).or_default();
        let add_lines = |content: &mut PageContent, lines: &[Vec<DVec3>]| {
            for line in lines {
                content.lines.extend(line.windows(2).map(|w| [flat(&w[0]), flat(&w[1])]));
            }
        };
        let add_meshes = |content: &mut PageContent, meshes: &[Mesh]| {
            for m in meshes {
                let loops = m.faces.iter().flat_map(|f| if f[2] == f[3] { vec![[f[0], f[1], f[2]]] } else { vec![[f[0], f[1], f[2]], [f[0], f[2], f[3]]] }).map(|t| t.iter().map(|&i| m.positions[i as usize]).map(|q| [q[0] * unit, q[1] * unit]).collect()).collect();
                content.fills.push(DraftFill { element: 0, loops, color: [color[0], color[1], color[2]] });
            }
        };
        match &object.geometry {
            Geometry::Lines(lines, _) => add_lines(content, lines),
            Geometry::Meshes(meshes, _) => add_meshes(content, meshes),
            Geometry::Drawing { lines, meshes, .. } => {
                add_lines(content, lines);
                add_meshes(content, meshes);
            }
            Geometry::Detail { boundary, view, scale, .. } => {
                content.details.push(PageDetail { name: if a.name.is_empty() { view.name.clone() } else { a.name.clone() }, boundary: boundary.iter().map(flat).collect(), view: view.clone(), scale: *scale });
            }
            _ => {
                *self.skipped.entry("objects on layouts not drawn on their sheet").or_default() += 1;
                return;
            }
        }
        content.objects += 1;
    }

    /// The layouts as sheets, each with its paper, what lies on it and its details as viewports
    /// showing the model as their view looks at it; those views are added to `views`.
    fn sheets(&mut self, views: &mut Vec<SavedView>) -> Vec<Sheet> {
        let origin = self.origin.unwrap_or(DVec3::ZERO);
        let meters = self.file.meters;
        let page_meters = self.file.page_meters;
        let mut pages: Vec<(&threedm_read::View, &threedm_read::Page)> = self.file.views.iter().filter_map(|v| v.page.as_ref().map(|p| (v, p))).collect();
        pages.sort_by_key(|(_, p)| p.number);
        let mut sheets = Vec::new();
        for (view, page) in pages {
            let content = self.pages.remove(&view.id).unwrap_or_default();
            if content.objects > 0 {
                *self.drawn.entry("objects on layouts").or_default() += content.objects;
            }
            let mut viewports = Vec::new();
            for d in content.details {
                let Some(rect) = d.boundary.iter().map(|&p| SheetRect { min: p, max: p }).reduce(SheetRect::union) else { continue };
                let v = &d.view;
                views.push(SavedView {
                    name: format!("{} – {}", view.name, d.name),
                    kind: ViewKind::ThreeD,
                    camera: Some(ViewCamera {
                        eye: (v.location * meters - origin).as_vec3().to_array(),
                        forward: v.direction.as_vec3().to_array(),
                        perspective: v.perspective,
                        fov_y: (v.perspective && v.fov_y > 0.0).then_some(v.fov_y as f32),
                    }),
                    clip: None,
                    plan: None,
                    level: None,
                    folder: vec![view.name.clone()],
                    style: None,
                    drawing: None,
                });
                // what a metre of the model is on the paper, and where the model's origin lies
                // on it: the camera's point in the middle of the detail
                let ratio = d.scale.map_or(0.0, |s| s * page_meters / meters) as f32;
                let right = v.direction.cross(v.up).normalize_or_zero();
                let at = v.location * meters - origin;
                let middle = [(rect.min[0] + rect.max[0]) / 2.0, (rect.min[1] + rect.max[1]) / 2.0];
                let shift = [at.dot(right) as f32 * ratio, at.dot(v.up) as f32 * ratio];
                viewports.push(SheetViewport {
                    title: d.name.clone(),
                    view: Some(views.len() - 1),
                    rect,
                    estimated: false,
                    scale: if ratio > 0.0 { format!("1:{}", (1.0 / ratio).round()) } else { String::new() },
                    detail: None,
                    label: None,
                    origin: [middle[0] - shift[0], middle[1] - shift[1]],
                    ratio,
                    drawing: None,
                    anchor: None,
                    turn: [1.0, 0.0],
                    draft: None,
                    written: None,
                    face: crate::typeface::Face::default(),
                    split: None,
                });
            }
            let paper = Some(SheetRect { min: [0.0, 0.0], max: [(page.width * 0.001) as f32, (page.height * 0.001) as f32] });
            let number = (page.number + 1).to_string();
            sheets.push(Sheet { lines: content.lines, fills: content.fills, ..Sheet::new(number, view.name.clone(), paper, viewports) });
        }
        // objects on a layout the file does not have
        let lost: usize = self.pages.values().map(|c| c.objects).sum();
        if lost > 0 {
            *self.skipped.entry("objects on layouts").or_default() += lost;
        }
        sheets
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
        let groups: Vec<&str> = a.groups.iter().filter_map(|g| self.file.groups.iter().find(|(i, _)| i == g)).map(|(_, n)| n.as_str()).collect();
        if !groups.is_empty() {
            p.insert("Groups".into(), groups.join(", ").into());
        }
        if !a.url.is_empty() {
            p.insert("URL".into(), a.url.clone().into());
        }
        if a.plot_weight_source == 1 {
            p.insert("Print width (mm)".into(), if a.plot_weight < 0.0 { "no print".to_string() } else { a.plot_weight.to_string() }.into());
        }
        if a.plot_color_source == 1 {
            let c = a.plot_color;
            p.insert("Print colour".into(), format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]).into());
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
        // Hidden objects use the remaining drawing budget and do not choose the origin before
        // visible geometry, even when they occur first in the file.
        let objects = file.objects.iter().filter(|o| o.attributes.visible && o.attributes.mode != 1)
            .chain(file.objects.iter().filter(|o| !o.attributes.visible || o.attributes.mode == 1));
        for object in objects {
            let a = &object.attributes;
            if a.mode == 3 {
                continue; // block definition geometry, drawn by its instances
            }
            if a.space == 2 {
                self.page_object(object);
                continue;
            }
            // hidden objects (Hide, or hidden by mode) are read too, onto hidden layers of their
            // own, so that they can be shown
            self.hiding = !a.visible || a.mode == 1;
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
                    self.hidden_objects += usize::from(drawn && self.hiding);
                }
                geometry => {
                    let kind = match geometry {
                        Geometry::Meshes(_, kind) | Geometry::Points { kind, .. } | Geometry::Lines(_, kind) | Geometry::Drawing { kind, .. } => *kind,
                        Geometry::ClippingPlane(_) => "Clipping plane",
                        _ => "",
                    };
                    let props = match geometry {
                        Geometry::ClippingPlane(p) => {
                            let user: Vec<_> = self.clip_props(p).into_iter().chain(object.user.iter().cloned()).collect();
                            self.props(a, kind, None, &user)
                        }
                        _ => self.props(a, kind, None, &object.user),
                    };
                    let color = self.paint(a, None).rgba();
                    let mut first = Some(props);
                    if self.draw(geometry, a.layer, root, color, id, &mut first) {
                        *self.drawn.entry(plural(kind)).or_default() += 1;
                        self.hidden_objects += usize::from(self.hiding);
                    }
                }
            }
        }
        for light in &file.lights {
            self.light(light, root);
        }
    }

    /// A light as an element: a small cross where it is, and where it shines a line (a spot's cone
    /// axis, a directional light's direction) or its shape (the line or rectangle of a linear or
    /// rectangular light), in its colour, with its values as properties. Lights that move with
    /// the camera and ambient light have no place in the model and are counted.
    fn light(&mut self, light: &threedm_read::Light, root: DMat4) {
        use threedm_read::LightKind;
        if light.camera {
            *self.skipped.entry("lights that move with the camera").or_default() += 1;
            return;
        }
        let p = light.location;
        let mut lines: Vec<Vec<DVec3>> = match light.kind {
            LightKind::Ambient | LightKind::Unknown => {
                *self.skipped.entry("ambient lights (no place)").or_default() += 1;
                return;
            }
            LightKind::Point => Vec::new(),
            LightKind::Spot | LightKind::Directional => vec![vec![p, p + light.direction]],
            LightKind::Linear => vec![vec![p, p + light.length]],
            LightKind::Rectangular => vec![vec![p, p + light.length, p + light.length + light.width, p + light.width, p]],
        };
        if !p.is_finite() {
            *self.skipped.entry("lights that could not be read").or_default() += 1;
            return;
        }
        let a = Attributes { id: light.id, name: light.name.clone(), layer: light.layer, visible: light.visible, ..Attributes::default() };
        let c = light.diffuse;
        let mut values = vec![
            ("Light".to_string(), format!("{:?}", light.kind)),
            ("On".to_string(), if light.on { "yes" } else { "no" }.to_string()),
            ("Intensity".to_string(), format!("{}", light.intensity)),
            ("Colour".to_string(), format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])),
        ];
        if light.kind == LightKind::Spot {
            values.push(("Spot angle (degrees)".into(), format!("{}", light.spot_angle)));
            values.push(("Hot spot".into(), format!("{}", light.hotspot)));
        }
        if light.watts > 0.0 {
            values.push(("Watts".into(), format!("{}", light.watts)));
        }
        let props = self.props(&a, "Light", None, &values);
        let id = self.next_id;
        self.next_id += 1;
        self.hiding = !light.visible;
        let color: Rgba = [c[0], c[1], c[2], 255];
        // a cross of 20 cm where it is
        let r = 0.1 / self.file.meters;
        for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
            lines.push(vec![p - axis * r, p + axis * r]);
        }
        let mut first = Some(props);
        if self.draw(&Geometry::Lines(lines, "Light"), light.layer, root, color, id, &mut first) {
            *self.drawn.entry("lights").or_default() += 1;
            self.hidden_objects += usize::from(self.hiding);
        }
        self.hiding = false;
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
            Geometry::ClippingPlane(p) => {
                // its rectangle, as Rhino draws it
                let ring = p.corners.iter().chain(&p.corners[..1]).copied().collect();
                self.draw(&Geometry::Lines(vec![ring], "Clipping plane"), layer, xform, color, id, props)
            }
            _ => false,
        }
    }

    /// The views saved by name, as saved 3D views with their camera (display coordinates).
    fn views(&self) -> Vec<SavedView> {
        let origin = self.origin.unwrap_or(DVec3::ZERO);
        self.file
            .named_views
            .iter()
            .filter(|v| v.direction != DVec3::ZERO && v.direction.is_finite() && v.location.is_finite())
            .map(|v| SavedView {
                name: v.name.clone(),
                kind: ViewKind::ThreeD,
                camera: Some(ViewCamera {
                    eye: (v.location * self.file.meters - origin).as_vec3().to_array(),
                    forward: v.direction.as_vec3().to_array(),
                    perspective: v.perspective,
                    fov_y: (v.perspective && v.fov_y.is_finite() && v.fov_y > 0.0).then_some(v.fov_y as f32),
                }),
                clip: None,
                plan: None,
                level: None,
                folder: Vec::new(),
                style: None,
                drawing: None,
            })
            .collect()
    }

    fn finish(mut self) -> (Vec<Layer>, Vec<(String, String)>, Option<SectionPlane>, Vec<SavedView>, Vec<Sheet>) {
        // first the layouts, which count what they hold
        let mut views = self.views();
        let sheets = self.sheets(&mut views);
        // the layers of hidden objects last
        let (meshes, points) = (std::mem::take(&mut self.meshes), std::mem::take(&mut self.points));
        let (shown, hidden): (Vec<_>, Vec<_>) = meshes.into_iter().chain(points).partition(|((_, hiding), _)| !hiding);
        let mut layers: Vec<Layer> = shown.into_iter().chain(hidden).map(|(_, l)| l).collect();
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
        if self.hidden_objects > 0 {
            info.push(("Rhino hidden objects".into(), format!("{} (on hidden layers of their own)", self.hidden_objects)));
        }

        let (section, clipping) = self.section();
        if clipping > 0 {
            let how = match (section.is_some(), clipping) {
                (true, 1) => "1, the section plane".to_string(),
                (true, n) => format!("{n}, the first the section plane"),
                (false, n) => format!("{n}, not cut (to a depth or of some objects only)"),
            };
            info.push(("Rhino clipping planes of the perspective view".into(), how));
        }
        (layers, info, section, views, sheets)
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
        "Clipping plane" => "clipping planes",
        "Light" => "lights",
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

fn convert(file: &File<'_>) -> (Vec<Layer>, Vec<(String, String)>, Option<SectionPlane>, Vec<SavedView>, Vec<Sheet>) {
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
            &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &i32b(0)),
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
        assert_eq!(names, ["Walls", "Walls::Glass", "Hidden::Off parent child", "Walls (points)", "Walls (hidden objects)"]);
        assert!(model.layers[4].hidden);
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
        // elements: slab, polysurface, extrusion, block instance, hidden-layer mesh, hidden object
        assert_eq!(model.props.len(), 6);
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
        assert_eq!(entry(&info, "Rhino objects drawn"), "1 block instances, 1 extrusions, 3 meshes, 1 point clouds, 1 polysurfaces");
        assert_eq!(entry(&info, "Rhino objects left out"), "1 SubD objects that could not be read, 1 curves, 1 objects on layouts");
    }

    /// A Rhino 4 file: 4-byte chunk lengths, the attributes layout before Rhino 5, an
    /// uncompressed (1.x) mesh, a Rhino 4 material, a Rhino 5 extrusion with its meshes in user
    /// data, metres.
    #[test]
    fn rhino4_layout() {
        let w = W { big: false };
        let layers = w.table(TCODE_LAYER_TABLE, &[w.layer(0, "Default", [0, 0, 0, 0], true, id(1), Uuid::default(), 0)]);
        let material = w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&id(9).0, &i32b(0), &wstr("Oak"), &[0; 16], &[0; 4], &[150, 100, 50, 0], &[0; 16], &f64s(&[1.0, 0.0, 0.0, 0.0]), &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &i32b(0))]));
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
        let glass = cat(&[&w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&0x0Cu32.to_le_bytes(), &i32b(0), &wstr("Glass")])), &[0; 16], &[0; 4], &[0, 0, 255, 0], &[0; 16], &f64s(&[1.0, 0.0, 0.0, 0.5]), &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &i32b(0))]);
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

    #[test]
    fn named_view_cameras_follow_units_and_the_local_origin() {
        let mut file = File::default();
        file.meters = 0.001;
        file.named_views.push(threedm_read::View {
            id: Uuid::default(),
            name: "Saved perspective".into(),
            perspective: true,
            location: DVec3::new(155_001_000.0, 463_002_000.0, 3_000.0),
            direction: DVec3::NEG_Z,
            up: DVec3::Y,
            fov_y: 0.75,
            height: 2_000.0,
            page: None,
        });
        let mut scene = Scene::new(&file);
        scene.origin = Some(DVec3::new(155_000.0, 463_000.0, 0.0));
        let views = scene.views();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].name, "Saved perspective");
        let camera = views[0].camera.as_ref().unwrap();
        assert_eq!(camera.eye, [1.0, 2.0, 3.0]);
        assert_eq!(camera.forward, [0.0, 0.0, -1.0]);
        assert!(camera.perspective);
        assert_eq!(camera.fov_y, Some(0.75));
    }

    #[test]
    fn visible_objects_keep_the_triangle_budget_and_origin_before_hidden_objects() {
        let object = |name: &str, hidden: bool, x: f32, y: f32| threedm_read::RhinoObject {
            attributes: Attributes { name: name.into(), visible: !hidden, ..Attributes::default() },
            geometry: Geometry::Meshes(vec![Mesh {
                positions: vec![[x, y, 0.0], [x + 1.0, y, 0.0], [x, y + 1.0, 0.0]],
                faces: vec![[0, 1, 2, 2]],
                triangles: 1,
                ..Mesh::default()
            }], "Mesh"),
            user: Vec::new(),
        };
        let mut file = File::default();
        file.meters = 1.0;
        file.objects = vec![object("Hidden", true, 1_000_000_000.0, 1_000_000_000.0), object("Visible", false, 155_000.25, 463_000.5)];
        let mut scene = Scene::new(&file);
        scene.triangles = MAX_TRIANGLES - 1;
        scene.run();
        let (layers, info, _, _, _) = scene.finish();
        let visible = layers.iter().find(|l| !l.hidden).unwrap();
        assert_eq!(visible.triangle_count(), 1);
        assert_eq!(visible.positions[..6], [0.25, 0.5, 0.0, 1.25, 0.5, 0.0]);
        assert_eq!(entry(&info, "Rhino world origin (metres)"), "155000, 463000, 0");
    }

    /// Clipping planes (`clipping.3dm`, from make_clipping.py, in millimetres): each is drawn as
    /// its rectangle; the one that clips the Perspective view (normal up, at 5 mm) is the section
    /// the file opens with, cutting away the top half of the 10 mm box; the one that clips the Top
    /// view only to a depth, and the one that is off, are not.
    #[test]
    fn the_clipping_plane_of_the_perspective_view_is_the_section() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/rhino");
        let (model, info) = load("clipping.3dm", std::fs::read(dir.join("clipping.3dm")).unwrap());
        assert_eq!(entry(&info, "Rhino objects drawn"), "3 clipping planes, 1 polysurfaces");
        assert_eq!(entry(&info, "Rhino clipping planes of the perspective view"), "1, the section plane");
        let plane = model.project.section.expect("a section");
        assert!((plane.normal - glam::Vec3::Z).length() < 1e-6 && (plane.offset - 0.005).abs() < 1e-7, "{plane:?}");
        assert!(plane.cuts(glam::Vec3::new(0.005, 0.005, 0.008)) && !plane.cuts(glam::Vec3::new(0.005, 0.005, 0.002)));
        let props: Vec<serde_json::Value> = model.props.values().map(|p| serde_json::from_str(p).unwrap()).filter(|p: &serde_json::Value| p["Type"] == "Clipping plane").collect();
        let clips = |name: &str| props.iter().find(|p| p["Name"] == name).unwrap_or_else(|| panic!("{name}"))["Clips"].as_str().unwrap().to_string();
        assert_eq!((clips("Cut"), clips("Slab"), clips("Off")), ("Perspective".into(), "Top".into(), "no view (off)".into()));
        assert!(props.iter().any(|p| p["Name"] == "Slab" && p["Clipping depth (m)"] == "0.002"));
        // each a closed rectangle of four lines (two indices each)
        let indices: usize = model.layers.iter().map(|l| l.line_elements.iter().map(|e| e.2 as usize).sum::<usize>()).sum();
        assert_eq!(indices, 3 * 4 * 2);
    }

    /// Object attributes (`attributes.3dm`, from make_attributes.py) as properties: the groups
    /// by name, the url, print colour and width set by object.
    #[test]
    fn groups_url_and_print_settings_are_properties() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/rhino");
        let (model, _) = load("attributes.3dm", std::fs::read(dir.join("attributes.3dm")).unwrap());
        let grouped: serde_json::Value = model.props.values().map(|p| serde_json::from_str(p).unwrap()).find(|p: &serde_json::Value| p["Name"] == "Grouped").expect("grouped");
        assert_eq!(grouped["Groups"], "Fence, North side");
        assert_eq!(grouped["URL"], "https://example.org/fence");
        assert_eq!(grouped["Print colour"], "#0A141E");
        assert_eq!(grouped["Print width (mm)"], "0.35");
    }

    /// Layouts (`layouts.3dm`, from make_layouts.py): "Plan" on A3 landscape with a Top detail at
    /// 1:100, a text and a line, and "View" on A4 portrait with a Perspective detail, as sheets in
    /// that order: paper, what lies on it, and each detail a viewport showing a view of its own.
    #[test]
    fn layouts_are_sheets_and_details_their_viewports() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/rhino");
        let (model, info) = load("layouts.3dm", std::fs::read(dir.join("layouts.3dm")).unwrap());
        assert!(!entry(&info, "Rhino objects left out").contains("layouts"), "{info:?}");
        let sheets = &model.project.sheets;
        assert_eq!(sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Plan", "View"]);
        let paper = |k: usize| sheets[k].paper.map(|p| [(p.max[0] * 1000.0).round(), (p.max[1] * 1000.0).round()]);
        assert_eq!((paper(0), paper(1)), (Some([420.0, 297.0]), Some([210.0, 297.0])));
        assert_eq!(sheets[0].viewports.len(), 1);
        assert_eq!(sheets[1].viewports.len(), 1);
        let plan = &sheets[0].viewports[0];
        assert_eq!(plan.scale, "1:100");
        let view = &model.views[plan.view.unwrap()];
        assert!(!view.camera.unwrap().perspective);
        assert!(model.views[sheets[1].viewports[0].view.unwrap()].camera.unwrap().perspective);
        assert!(!sheets[0].lines.is_empty() && !sheets[0].fills.is_empty(), "the line and the text on the plan");
    }

    /// Lights (`lights.3dm`, from make_lights.py): the point, spot, directional, linear and
    /// rectangular light are drawn as elements with their values (the hidden directional light on
    /// a hidden layer); the ambient light and the headlight have no place and are counted.
    #[test]
    fn lights_are_drawn_where_they_are() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/rhino");
        let (model, info) = load("lights.3dm", std::fs::read(dir.join("lights.3dm")).unwrap());
        assert!(entry(&info, "Rhino objects drawn").contains("5 lights"), "{info:?}");
        assert_eq!(entry(&info, "Rhino objects left out"), "1 ambient lights (no place), 1 lights that move with the camera");
        let lights: Vec<serde_json::Value> = model.props.values().map(|p| serde_json::from_str(p).unwrap()).filter(|p: &serde_json::Value| p["Type"] == "Light").collect();
        assert_eq!(lights.len(), 5);
        let spot = lights.iter().find(|p| p["Name"] == "Spot").expect("spot");
        assert_eq!(spot["Light"], "Spot");
        assert!(spot.get("Spot angle (degrees)").is_some() && spot.get("Intensity").is_some());
        assert!(model.layers.iter().any(|l| l.hidden && l.name.ends_with("(hidden objects)")));
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
    /// a block, points, a SubD, a curve, a text, a hatch, and a hidden object (on a hidden layer). Both
    /// read the same.
    #[test]
    fn rhino9_layers_materials_colours_and_user_text() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/rhino");
        for (file, meshed) in [("everything_meshes.3dm", 3), ("everything_small.3dm", 15)] {
            let (model, info) = load(file, std::fs::read(dir.join(file)).unwrap());
            assert_eq!(entry(&info, "Rhino file version"), "Rhino 9 (archive version 90)");
            assert_eq!(entry(&info, "Rhino objects drawn"), "1 SubD objects, 1 block instances, 1 curves, 1 extrusions, 1 hatches, 1 meshes, 1 point clouds, 1 points, 11 polysurfaces, 1 surfaces, 1 texts", "{file}");
            assert_eq!(entry(&info, "Rhino objects left out"), "", "{file}");
            assert_eq!(entry(&info, "Rhino hidden objects"), "1 (on hidden layers of their own)", "{file}");
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
            assert!(elements.contains_key("Hidden box"));
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
