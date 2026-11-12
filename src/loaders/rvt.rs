//! Revit (.rvt) via the OpenAEC rvt reader: one layer per category batch, with material colours
//! per vertex, the drawn elements (for picking) and their properties.

use crate::loaders::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{
    y_up_to_z_up, GridLine, Later, Layer, PlanRange, ProjectData, Room, SavedView, Schedule, ScheduleRow, Sheet, SheetRect,
    SheetViewport, SourceWall, ViewCamera, ViewKind,
};
use rvt_model::{FieldValue, ObjectView};
use std::collections::HashMap;
use crate::section::SectionBox;
use glam::Vec3;

pub struct RvtLoader;

impl Loader for RvtLoader {
    fn name(&self) -> &'static str {
        "Revit"
    }

    fn featured(&self) -> Option<u8> {
        Some(1)
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rvt", "rfa"]
    }

    fn source_y_up(&self) -> bool {
        true
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|l| l.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        // the Revit reader opens the file itself (a compound file, read in parts)
        let path = source.path().ok_or("Revit files open in the desktop app only")?;
        let doc = rvt_read::Document::open(path).map_err(|e| format!("Cannot read Revit file: {e}"))?;
        progress.set(Phase::Convert);
        // `scene` returns view coordinates: metres, Y up (x, z, -y); the walls the Modeller can
        // take over are read beside it (a second or so on a large project)
        let (scene, walls) = std::thread::scope(|s| {
            let walls = s.spawn(|| source_walls(&doc));
            let scene = rvt_export::mesh::scene(&doc);
            (scene, walls.join().unwrap_or_default())
        });
        let infos = rvt_export::views::views(&doc);
        // each view in the folders the file's browser orders it in
        let folders = super::rvt_browser::folders(&doc, &infos);
        let in_folder = |(id, mut view): (u64, SavedView)| {
            view.folder = folders.get(&id).cloned().unwrap_or_default();
            (id, view)
        };
        let (view_ids, views): (Vec<u64>, Vec<SavedView>) = saved_views(&infos).into_iter().map(in_folder).unzip();
        let mut project = project_data(&doc, &scene.levels);
        project.walls = walls;
        // how a cut through each material is hatched
        project.cut_patterns = super::rvt_materials::cut_patterns(&doc);
        let layers = scene
            .batches
            .into_iter()
            .map(|b| {
                let mut positions = b.positions;
                let mut normals = b.normals;
                y_up_to_z_up(&mut positions);
                y_up_to_z_up(&mut normals);
                // RGBA per vertex when the batch has material colours
                let colors = (!b.colors.is_empty() && b.colors.len() == positions.len() / 3 * 4)
                    .then(|| b.colors.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect());
                Layer {
                    name: b.name,
                    color: b.color,
                    positions,
                    normals,
                    colors,
                    indices: b.indices,
                    elements: b.elements,
                    props: b.props,
                    hidden: false,
                    points: false,
                }
            })
            .collect::<Vec<Layer>>();
        // the plans, sections and elevations with their texts and dimensions: the project's drawings
        let file = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        // the tags of its views, with the labels and lines of their types
        let mut types = super::rvt_drawings::Types::new(&doc);
        let tags = super::rvt_tags::tags(&doc, &mut types);
        // the detail components of its views, as the lines of their families
        let details = super::rvt_tags::details(&doc, &mut types);
        drop(types);
        project.drawings = super::rvt_drawings::drawings(&doc, &infos, &file, &tags, &details);
        // the phase and phase filter of every view, and the phases of the elements
        let ids: Vec<u64> = infos.iter().map(|v| v.id).collect();
        project.phasing = super::rvt_phases::phasing(&doc, &ids, &|view| crate::document::uuid(&format!("rvt/{file}/view/{view}")));
        // the legends and drafting views: drawings without a model
        project.drafts = super::rvt_drafts::drafts(&doc, &tags, &details);
        project.drawn = super::rvt_props::drawn(&doc, &project.drafts);
        project.sheets = sheets(&doc, &view_ids, &views, &layers, &file, &project, &infos);
        // the schedules take seconds on a large project: computed once the model shows
        project.schedules = Later::new(move || schedules(&doc));
        Ok(Loaded { structural: Vec::new(), origin: None, layers, views, project })
    }
}

/// Feet to metres.
const FT: f64 = 0.3048;
/// Revit's categories of doors and windows.
const DOORS: i32 = -2000023;
const WINDOWS: i32 = -2000014;

/// The straight walls of the project that the Modeller can take over ([`SourceWall`]), by
/// element id: the location line, base and height of the wall, its type's thickness, the name of
/// its base level, and the doors and windows it hosts as openings ([`cut_of`]). Curved, stacked,
/// in-place and curtain walls, walls without a thickness or a height, and walls with a door or
/// window whose extent along them cannot be told, are left out: they stay meshes.
fn source_walls(doc: &rvt_read::Document) -> HashMap<u64, SourceWall> {
    let levels: HashMap<i64, String> = doc.levels().into_iter().map(|l| (l.id as i64, l.name)).collect();
    let mut walls = HashMap::new();
    for w in doc.walls() {
        if w.curve.is_some() || w.in_place || w.stacked || w.stacked_parent.is_some() || w.curtain_panel {
            continue;
        }
        let (Some(thickness), Some(height)) = (doc.type_thickness(w.wall_type), w.height) else { continue };
        if !(thickness.is_finite() && height.is_finite() && thickness > 0.0 && height > 0.0) {
            continue;
        }
        let (start, end) = (glam::DVec2::new(w.p0[0], w.p0[1]) * FT, glam::DVec2::new(w.p1[0], w.p1[1]) * FT);
        if start.distance(end) < 1e-3 {
            continue;
        }
        // as the mesher stands it: on its base level and offset, else on its line's height
        let base = doc.level_elevation(w.base_level).map_or(w.p0[2], |z| z + w.base_offset) * FT;
        let wall = SourceWall { start, end, base, height: height * FT, thickness: thickness * FT, openings: Vec::new(), level: levels.get(&w.base_level).cloned() };
        walls.insert(w.id, wall);
    }
    // a door's or window's category is its family's, which its instance record carries
    let doors_windows: HashMap<u64, rvt_model::Instance> = doc.instances().into_iter().filter(|i| matches!(i.cat, DOORS | WINDOWS)).map(|i| (i.id, i)).collect();
    // their types' Width (−1001301) and Height (−1001300), where both are given
    let types: HashMap<u64, rvt_model::ValueSet> = doc.type_values_by_chain().into_iter().flat_map(|(_, _, m)| m).collect();
    let size = |symbol: Option<u64>| -> Option<(f64, f64)> {
        let rows = &types.get(&symbol?)?.rows;
        let value = |param: i64| rows.iter().find(|r| r.param == param).map(|r| r.num).filter(|v| *v > 1e-6);
        Some((value(-1001301)?, value(-1001300)?))
    };
    let mut unknown = std::collections::HashSet::new();
    for h in doc.hosted() {
        let Some(host) = h.host.and_then(|x| u64::try_from(x).ok()) else { continue };
        let Some(wall) = walls.get_mut(&host) else { continue };
        let Some(instance) = doors_windows.get(&h.id) else { continue };
        let hole = Hole {
            bb: doc.element(h.id).and_then(|e| e.bbox),
            size: size(instance.symbol),
            origin: instance.origin,
            bottom: doc.level_elevation(h.level).map(|z| z + h.sill.unwrap_or(0.0)),
        };
        match cut_of(wall, &hole) {
            Cut::Opening(o) => wall.openings.push(o),
            Cut::Nothing => {}
            Cut::Unknown => {
                unknown.insert(host);
            }
        }
    }
    walls.retain(|id, _| !unknown.contains(id));
    for wall in walls.values_mut() {
        wall.openings = merged(std::mem::take(&mut wall.openings));
    }
    walls
}

/// Openings that overlap (a door's frame, leaf and lining are elements of their own, each with
/// its box) as one: the extent of them all, as the mesher's cuts of them make one hole.
fn merged(mut openings: Vec<bim_core::Opening>) -> Vec<bim_core::Opening> {
    let overlap = |a: &bim_core::Opening, b: &bim_core::Opening| {
        a.along < b.along + b.width && b.along < a.along + a.width && a.sill < b.sill + b.height && b.sill < a.sill + a.height
    };
    let mut out: Vec<bim_core::Opening> = Vec::new();
    while let Some(mut o) = openings.pop() {
        // grow it by every opening it overlaps, until none does
        while let Some(k) = openings.iter().chain(out.iter()).position(|other| overlap(&o, other)) {
            let other = if k < openings.len() { openings.swap_remove(k) } else { out.swap_remove(k - openings.len()) };
            let (from, to) = (o.along.min(other.along), (o.along + o.width).max(other.along + other.width));
            let (bottom, top) = (o.sill.min(other.sill), (o.sill + o.height).max(other.sill + other.height));
            o = bim_core::Opening { along: from, sill: bottom, width: to - from, height: top - bottom };
        }
        out.push(o);
    }
    out.sort_by(|a, b| a.along.total_cmp(&b.along).then(a.sill.total_cmp(&b.sill)));
    out
}

/// What a door or window tells of the hole it makes in its wall, in feet and the file's frame.
struct Hole {
    /// Its box (xmin, ymin, zmin, xmax, ymax, zmax), if stored.
    bb: Option<[f64; 6]>,
    /// Its type's width and height, when the type gives both.
    size: Option<(f64, f64)>,
    /// Its insertion point: the middle of its width.
    origin: Option<[f64; 3]>,
    /// The height its bottom stands at: its level's elevation plus its sill.
    bottom: Option<f64>,
}

/// What a door or window does to its wall.
#[derive(Debug, PartialEq)]
enum Cut {
    Opening(bim_core::Opening),
    /// It takes nothing from the wall: a symbol without height (a swing drawn in plan), or a hole
    /// beside the wall.
    Nothing,
    /// Its extent along the wall cannot be told: the wall stays a mesh.
    Unknown,
}

/// The opening a door or window makes in `wall`, within the wall:
/// - by its type, where the type gives a width and a height: that width about its insertion
///   point, that height from its level plus its sill. A type's width is its nominal width (often
///   the clear opening), so the wall may reach a few centimetres into the door's frame;
/// - else by its box, as the mesher cuts it: the box's extent along the axis and its height. A
///   box turned with the wall is `w·c + t·s` wide in x and `w·s + t·c` in y, for a width `w` along
///   the wall, a depth `t` across it and the axis direction's `c = |cos|`, `s = |sin|`; the width
///   follows from both, except near 45°, where the box does not tell it.
fn cut_of(wall: &SourceWall, hole: &Hole) -> Cut {
    let length = wall.start.distance(wall.end);
    let d = (wall.end - wall.start) / length;
    let (from, to, bottom, top) = if let (Some((w, h)), Some(o), Some(z)) = (hole.size, hole.origin, hole.bottom) {
        let middle = (glam::DVec2::new(o[0], o[1]) * FT - wall.start).dot(d);
        (middle - w * FT / 2.0, middle + w * FT / 2.0, z * FT, (z + h) * FT)
    } else if let Some(bb) = hole.bb {
        let bb = bb.map(|v| v * FT);
        let (c, s) = (d.x.abs(), d.y.abs());
        let (ex, ey) = (bb[3] - bb[0], bb[4] - bb[1]);
        let width = if s < 1e-6 {
            ex
        } else if c < 1e-6 {
            ey
        } else {
            let det = c * c - s * s;
            if det.abs() < 0.2 {
                return Cut::Unknown;
            }
            (ex * c - ey * s) / det
        };
        let middle = (glam::DVec2::new(bb[0] + bb[3], bb[1] + bb[4]) / 2.0 - wall.start).dot(d);
        (middle - width / 2.0, middle + width / 2.0, bb[2], bb[5])
    } else {
        return Cut::Unknown;
    };
    let (from, to) = (from.max(0.0), to.min(length));
    let (bottom, top) = ((bottom - wall.base).max(0.0), (top - wall.base).min(wall.height));
    if to - from > 1e-3 && top - bottom > 1e-3 {
        Cut::Opening(bim_core::Opening { along: from, sill: bottom, width: to - from, height: top - bottom })
    } else {
        Cut::Nothing
    }
}

/// The schedules as the viewer shows them: templates left out, the shown columns only.
fn schedules(doc: &rvt_read::Document) -> Vec<Schedule> {
    rvt_export::schedules::schedules(doc)
        .into_iter()
        .filter(|s| !s.is_template)
        .map(|s| {
            let shown: Vec<usize> = (0..s.columns.len()).filter(|&i| !s.columns[i].hidden).collect();
            // the image a cell shows: what the column's parameter names for the row's first element
            let parameters: Vec<i64> = shown.iter().map(|&i| s.columns[i].parameter).collect();
            let image = |element: u64, parameter: i64| {
                let file = super::rvt_images::of_parameter(doc, element, parameter)?;
                let pixels = image::load_from_memory(&file).ok()?.to_rgba8();
                Some(iced::widget::image::Handle::from_rgba(pixels.width(), pixels.height(), pixels.into_raw()))
            };
            Schedule {
                name: s.name,
                title: s.title,
                columns: shown.iter().map(|&i| s.columns[i].heading.clone()).collect(),
                rows: s
                    .rows
                    .into_iter()
                    .map(|r| ScheduleRow {
                        images: {
                            let first = r.elements.first().copied().filter(|_| !r.total);
                            let all: Vec<_> = parameters.iter().map(|&p| image(first?, p)).collect();
                            if all.iter().any(Option::is_some) { all } else { Vec::new() }
                        },
                        cells: shown.iter().map(|&i| r.cells.get(i).cloned().flatten()).collect(),
                        elements: r.elements,
                        total: r.total,
                    })
                    .collect(),
                computed: s.computed,
            }
        })
        .collect()
}

/// A parameter value as stored on an element (internal units: feet, square feet, radians).
#[derive(Debug, Clone, PartialEq)]
enum ParamValue {
    Number(f64),
    Integer(i64),
    Text(String),
    Element(i64),
}

/// The parameter values stored on an element: its four value sets (number, integer, text,
/// element id), keyed by parameter id (negative: Revit's built-in parameters; positive: the
/// ParameterElement of a project, shared or family parameter). Only values set on the element
/// are stored; computed ones (length, area, volume) are not.
fn param_values(o: &ObjectView) -> Vec<(i64, ParamValue)> {
    let mut out = Vec::new();
    for (field, kind) in [("m_pParamValueSetDouble", 0), ("m_pParamValueSetInt", 1), ("m_pParamValueSetAString", 2), ("m_pParamValueSetElementId", 3)] {
        let Some(list) = o.get(field).and_then(|v| o.deref(v)).and_then(|set| set.get("m_paramSet")).and_then(FieldValue::as_list) else {
            continue;
        };
        for entry in list.iter().filter_map(FieldValue::as_object) {
            let (Some(id), Some(v)) = (entry.get("m_paramId").and_then(FieldValue::element), entry.get("m_value")) else { continue };
            let value = match kind {
                0 => v.as_f64().map(ParamValue::Number),
                1 => v.as_i64().map(ParamValue::Integer),
                2 => v.as_str().map(|s| ParamValue::Text(s.to_string())),
                _ => v.element().map(ParamValue::Element),
            };
            if let Some(value) = value {
                out.push((id, value));
            }
        }
    }
    out
}

/// The caption and spec (`autodesk.spec.aec:length-1.0.0`, …) of a project, shared or family
/// parameter, cached.
fn param_def(doc: &rvt_read::Document, cache: &mut HashMap<i64, Option<(String, String)>>, id: i64) -> Option<(String, String)> {
    if id <= 0 {
        return None;
    }
    cache
        .entry(id)
        .or_insert_with(|| {
            let o = doc.object(id as u64)?;
            let caption = o.path(&["m_pParamDef", "m_caption"])?.as_str()?.to_string();
            let spec = o.path(&["m_pParamDef", "m_specTypeId", "m_typeId"]).and_then(FieldValue::as_str).unwrap_or_default().to_string();
            Some((caption, spec))
        })
        .clone()
}

/// A value for display in the unit of its spec (lengths in mm, areas in m², volumes in m³,
/// angles in degrees, yes/no).
fn format_value(value: &ParamValue, spec: &str) -> String {
    let kind = spec.split(':').nth(1).and_then(|k| k.split('-').next()).unwrap_or_default();
    match value {
        ParamValue::Number(v) => match kind {
            "length" => format!("{:.0} mm", v * 304.8),
            "area" => format!("{:.2} m²", v * 0.092_903_04),
            "volume" => format!("{:.3} m³", v * 0.028_316_846_592),
            "angle" => format!("{:.2}°", v.to_degrees()),
            _ => format!("{v}"),
        },
        ParamValue::Integer(v) if kind == "bool" || spec.contains("spec.bool") => (if *v != 0 { "ja" } else { "nee" }).to_string(),
        ParamValue::Integer(v) => v.to_string(),
        ParamValue::Text(t) => t.clone(),
        ParamValue::Element(e) => format!("#{e}"),
    }
}

/// Project information, levels, phases, grids and rooms: the few elements that describe the
/// project rather than draw it.
fn project_data(doc: &rvt_read::Document, levels: &[(String, f32)]) -> ProjectData {
    const FT: f32 = 0.3048;
    let mut out = ProjectData::default();
    let mut defs = HashMap::new();

    // Project information: the built-in values with known ids (ProjectInfo of 3077: number
    // "3077", name, client, status), then the project's own parameters
    if let Some(o) = doc.object_ids_of_class("ProjectInfo").first().and_then(|&id| doc.object(id)) {
        let mut own = Vec::new();
        for (id, value) in param_values(&o) {
            let key = match id {
                -1006316 => "number",
                -1006317 => "name",
                -1006319 => "client",
                -1006320 => "status",
                id if id > 0 => {
                    if let Some((caption, spec)) = param_def(doc, &mut defs, id) {
                        let v = format_value(&value, &spec);
                        if !v.is_empty() {
                            own.push((caption, v));
                        }
                    }
                    continue;
                }
                _ => continue,
            };
            let v = format_value(&value, "");
            if !v.is_empty() {
                out.info.push((key.to_string(), v));
            }
        }
        let order = ["number", "name", "client", "status"];
        out.info.sort_by_key(|(k, _)| order.iter().position(|o| o == k));
        own.sort();
        out.info.extend(own);
    }

    let mut lv: Vec<(String, f32)> = levels.to_vec();
    lv.sort_by(|a, b| a.1.total_cmp(&b.1));
    lv.dedup();
    out.levels = lv;

    out.phases = doc
        .object_ids_of_class("ProjectPhase")
        .into_iter()
        .filter_map(|id| doc.object_root(id))
        .filter_map(|o| o.get("m_name").and_then(FieldValue::as_str).map(str::to_string))
        .collect();

    // Grids: the line of the grid's vertical plane (origin + u·xVec over the plane's extents),
    // drawn at the level nearest the project zero, as on the ground floor
    let ground = out.levels.iter().map(|l| l.1).min_by(|a, b| a.abs().total_cmp(&b.abs())).unwrap_or(0.0);
    for id in doc.object_ids_of_class("Grid") {
        let Some(o) = doc.object(id) else { continue };
        let Some(name) = o.get("m_text").and_then(FieldValue::as_str) else { continue };
        let Some(plane) = o.get("m_pSurface").and_then(|v| o.deref(v)).filter(|p| p.class == "Plane") else { continue };
        let vec3 = |name: &str| -> Option<[f64; 3]> {
            let l = plane.get(name)?.as_list()?;
            Some([l.first()?.as_f64()?, l.get(1)?.as_f64()?, l.get(2)?.as_f64()?])
        };
        let (Some(origin), Some(x)) = (vec3("m_origin"), vec3("m_xVec")) else { continue };
        let Some(corners) = plane.get("m_Envelope").and_then(FieldValue::as_object).and_then(|e| e.get("m_corners")).and_then(FieldValue::as_list) else {
            continue;
        };
        let u = |i: usize| corners.get(i).and_then(FieldValue::as_list).and_then(|c| c.first()).and_then(FieldValue::as_f64);
        let (Some(u0), Some(u1)) = (u(0), u(1)) else { continue };
        let at = |u: f64| [((origin[0] + u * x[0]) as f32) * FT, ((origin[1] + u * x[1]) as f32) * FT, ground];
        out.grids.push(GridLine { name: name.to_string(), a: at(u0), b: at(u1) });
    }
    out.grids.sort_by(|a, b| a.name.cmp(&b.name));

    // Rooms: number and name, level, and the project's own room parameters
    let level_names: HashMap<i64, String> = doc
        .object_ids_of_class("Level")
        .into_iter()
        .filter_map(|id| Some((id as i64, doc.object_root(id)?.get("m_text")?.as_str()?.to_string())))
        .collect();
    for id in doc.object_ids_of_class("RoomElem") {
        let Some(o) = doc.object(id) else { continue };
        let mut room = Room { level: o.get("m_levelId").and_then(FieldValue::element).and_then(|l| level_names.get(&l).cloned()), ..Room::default() };
        for (pid, value) in param_values(&o) {
            match pid {
                -1006900 => room.name = format_value(&value, ""),
                -1006901 => room.number = format_value(&value, ""),
                pid if pid > 0 => {
                    if let Some((caption, spec)) = param_def(doc, &mut defs, pid) {
                        let v = format_value(&value, &spec);
                        if !v.is_empty() {
                            room.extra.push((caption, v));
                        }
                    }
                }
                _ => {}
            }
        }
        if !room.name.is_empty() || !room.number.is_empty() {
            out.rooms.push(room);
        }
    }
    out.rooms.sort_by(|a, b| natural_key(&a.number).cmp(&natural_key(&b.number)).then(a.name.cmp(&b.name)));
    out
}

/// Sort key that orders "2" before "10".
fn natural_key(s: &str) -> (u64, String) {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    (digits.parse().unwrap_or(u64::MAX), s.to_string())
}

/// The model views of the file (beta) in world coordinates: the mesh is in Revit's model
/// coordinates scaled from feet to metres, Z up, so the views convert the same way.
fn saved_views(infos: &[rvt_export::views::ViewInfo]) -> Vec<(u64, SavedView)> {
    use rvt_export::views::{OrientedBox, ViewKind as K};
    const FT: f32 = 0.3048;
    let v3 = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let frame = |b: &OrientedBox| SectionBox::from_frame(v3(b.origin) * FT, b.basis.map(v3), v3(b.min) * FT, v3(b.max) * FT);
    // ±1e9 ft means unlimited
    let limited = |v: Option<f64>| v.filter(|v| v.abs() < 1e8);
    infos
        .into_iter()
        .filter_map(|v| {
            let kind = match &v.kind {
                K::ThreeD => ViewKind::ThreeD,
                k if k.is_plan() => ViewKind::Plan,
                K::Section => ViewKind::Section,
                K::Elevation => ViewKind::Elevation,
                K::Detail => ViewKind::Detail,
                _ => return None,
            };
            let mut out = SavedView { folder: Vec::new(), name: v.name.clone(), kind, camera: None, clip: None, plan: None, level: v.level_name.clone() };
            match kind {
                ViewKind::ThreeD => {
                    out.camera = v.camera.as_ref().map(|c| ViewCamera {
                        eye: (v3(c.eye) * FT).to_array(),
                        forward: v3(c.forward).normalize_or(Vec3::NEG_Z).to_array(),
                        perspective: c.perspective,
                        fov_y: c.fov.map(|f| f[1] as f32),
                    });
                    out.clip = v.section_box.as_ref().and_then(frame);
                }
                ViewKind::Plan => {
                    let level = v.level.map(|l| l.1);
                    let range = v.view_range.as_ref();
                    let cut = limited(range.and_then(|r| r.cut)).or(level.zip(v.cut_offset).map(|(l, c)| l + c)).or(level.map(|l| l + 4.0))?;
                    let ceiling = v.kind == K::CeilingPlan;
                    // a structural plan can look up too: its view depth then lies above its cut
                    let above = limited(range.and_then(|r| r.depth)).filter(|depth| !ceiling && *depth > cut + 1e-6);
                    let (low, high) = if let Some(depth) = above {
                        (cut, depth + 0.01)
                    } else if ceiling {
                        (cut, limited(range.and_then(|r| r.top)).unwrap_or(cut + 10.0))
                    } else {
                        // down to the view depth, so the floor below the cut shows
                        let low = limited(range.and_then(|r| r.depth)).or(limited(range.and_then(|r| r.bottom))).or(level).unwrap_or(cut - 10.0);
                        (low - 0.01, cut)
                    };
                    out.plan = Some(PlanRange { low: low as f32 * FT, high: high as f32 * FT, from_below: ceiling || above.is_some() });
                }
                _ => {
                    let crop = v.crop_box.as_ref()?;
                    out.camera = Some(ViewCamera {
                        eye: (v3(crop.origin) * FT).to_array(),
                        forward: v3(crop.basis[2]).normalize_or(Vec3::Y).to_array(),
                        perspective: false,
                        fov_y: None,
                    });
                    out.clip = frame(crop);
                }
            }
            Some((v.id, out))
        })
        .collect()
}

/// The sheets (beta) in sheet metres. The paper comes from the title block's size: its
/// insertion point is the paper's lower right corner (3077 R26), else another corner or the
/// centre, whichever holds the most viewports. A viewport without an active crop region gets the
/// size of the building as its view sees it (`estimated`, centred on its position).
fn sheets(doc: &rvt_read::Document, view_ids: &[u64], views: &[SavedView], layers: &[Layer], file: &str, project: &ProjectData, infos: &[rvt_export::views::ViewInfo]) -> Vec<Sheet> {
    let mut types = super::rvt_drawings::Types::new(doc);
    // the schedules placed on the sheets, as tables
    let mut tables = super::rvt_schedules::placed(doc);
    let anchors: HashMap<u64, ([f64; 2], [f64; 2])> = infos.iter().filter_map(|v| Some((v.id, super::rvt_drawings::view_origin(v)?))).collect();
    // the views the document draws, and the legends and drafting views, by their element
    let drawings: std::collections::HashSet<&str> = project.drawings.iter().map(|(path, _)| path.as_str()).collect();
    let drafts: HashMap<u64, usize> = project.drafts.iter().enumerate().map(|(k, d)| (d.id, k)).collect();
    use rvt_export::views::ViewKind as K;
    const FT: f32 = 0.3048;
    let index: HashMap<u64, usize> = view_ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    // the building without its site: the terrain reaches far beyond what a view shows
    let mut model = crate::model::Bounds::empty();
    for p in layers.iter().filter(|l| !l.name.contains("Topograph")).flat_map(|l| l.positions.chunks_exact(3)) {
        model.extend([p[0], p[1], p[2]]);
    }
    let [dx, dy, dz] = if model.is_valid() { model.size() } else { [10.0; 3] };
    // a placed title block draws its own variant of its type, with what is switched on for it
    let block_kind = |t: &rvt_export::sheets::TitleBlock| {
        let own = doc.object(t.id).and_then(|o| u64::try_from(o.path(&["m_pInstanceInfo", "m_symbolId"])?.element()?).ok());
        own.or(t.symbol)
    };
    rvt_export::sheets::sheets(doc)
        .into_iter()
        .map(|s| {
            let viewports: Vec<SheetViewport> = s
                .viewports
                .iter()
                .map(|v| {
                    let view = index.get(&v.view).copied();
                    let position = [v.position[0] as f32 * FT, v.position[1] as f32 * FT];
                    let scale = v.scale as f32;
                    // a legend or drafting view covers what is drawn in it
                    let drawn = drafts.get(&v.view).and_then(|&k| project.drafts[k].extent()).map(|e| SheetRect { min: [position[0] + e.min[0] * scale, position[1] + e.min[1] * scale], max: [position[0] + e.max[0] * scale, position[1] + e.max[1] * scale] });
                    let (rect, estimated) = match (v.rect, drawn) {
                        (Some([lo, hi]), _) => (SheetRect { min: lo.map(|x| x as f32 * FT), max: hi.map(|x| x as f32 * FT) }, false),
                        (None, Some(drawn)) => (drawn, false),
                        (None, None) => {
                            // the model as the view sees it: from above for a plan, across the view
                            // direction (and the full height) for the others
                            let seen = match (&v.view_kind, view.and_then(|i| views[i].camera.as_ref())) {
                                (k, _) if k.is_plan() => [dx, dy],
                                (_, Some(c)) => [dx * c.forward[1].abs() + dy * c.forward[0].abs(), dz],
                                (K::ThreeD, None) => [dx.max(dy), dz],
                                // legends and drafting views: a label-sized box
                                _ => [0.12 / scale, 0.08 / scale],
                            };
                            (SheetRect::around(position, [seen[0] * scale, seen[1] * scale]), true)
                        }
                    };
                    // the title as the type of the viewport writes it: its name and scale in one, or none
                    let ratio_text = format!("1:{}", (1.0 / v.scale).round());
                    let written = super::rvt_tags::viewport_title(doc, &mut types, v.id, v.view, &v.view_name, &format!("1 : {}", (1.0 / v.scale).round()), v.detail_number.as_deref());
                    SheetViewport {
                        title: v.view_name.clone(),
                        written,
                        view,
                        rect,
                        estimated,
                        scale: ratio_text,
                        detail: v.detail_number.clone(),
                        label: v.label_origin.map(|p| p.map(|x| x as f32 * FT)),
                        origin: position,
                        ratio: scale,
                        drawing: Some(crate::document::uuid(&format!("rvt/{file}/view/{}", v.view))).filter(|path| drawings.contains(path.as_str())),
                        anchor: anchors.get(&v.view).map(|a| a.0),
                        turn: anchors.get(&v.view).map_or([1.0, 0.0], |a| a.1),
                        draft: drafts.get(&v.view).copied(),
                    }
                })
                .collect();
            let paper = s.title_block.as_ref().and_then(|t| {
                let [w, h] = t.size?.map(|x| x as f32 * FT);
                let [ox, oy] = t.origin.map(|x| x as f32 * FT);
                let corners = [[ox - w, oy], [ox, oy], [ox - w, oy - h], [ox, oy - h], [ox - w / 2.0, oy - h / 2.0]];
                let held = |min: [f32; 2]| {
                    let r = SheetRect { min, max: [min[0] + w, min[1] + h] };
                    viewports.iter().filter(|v| r.contains(v.rect.center())).count()
                };
                // the first corner wins a tie: the observed lower right insertion point
                let min = corners.into_iter().rev().max_by_key(|&c| held(c))?;
                Some(SheetRect { min, max: [min[0] + w, min[1] + h] })
            });
            // the title block as its family draws it, from where it is placed
            let block = s.title_block.as_ref().and_then(|t| Some((t, block_kind(t)?))).map(|(t, kind)| {
                let (lines, texts) = super::rvt_tags::title_block(doc, &mut types, kind, s.id, &s.number, &s.name);
                let on = |p: [f64; 2]| [(t.origin[0] * 0.3048 + p[0]) as f32, (t.origin[1] * 0.3048 + p[1]) as f32];
                let texts = texts.into_iter().map(|x| {
                    let align = x.align.clamp(0, 2) as usize;
                    let at = on([x.at[0] - [0.0, x.width / 2.0, x.width][align], x.at[1]]);
                    crate::model::DraftText { element: t.id, at, width: x.width as f32, height: x.height() as f32, size: x.em as f32, align: align as u8, angle: x.angle as f32, face: x.face.clone(), text: x.wording }
                });
                (lines.into_iter().map(|l| [on(l[0]), on(l[1])]).collect(), texts.collect())
            });
            let mut sheet = Sheet::new(s.number, s.name, paper, viewports);
            if let Some((lines, texts)) = block {
                (sheet.lines, sheet.texts) = (lines, texts);
            }
            // the regions of its title block filled in one colour
            if let Some((t, kind)) = s.title_block.as_ref().and_then(|t| Some((t, block_kind(t)?))) {
                let on = |p: [f64; 2]| [(t.origin[0] * 0.3048 + p[0]) as f32, (t.origin[1] * 0.3048 + p[1]) as f32];
                let fills = super::rvt_tags::title_block_fills(doc, &mut types, kind).into_iter();
                sheet.line_colors = super::rvt_tags::title_block_line_colors(doc, &mut types, kind);
                sheet.fills = fills.map(|(loops, color)| crate::model::DraftFill { element: t.id, loops: loops.iter().map(|l| l.iter().map(|&p| on(p)).collect()).collect(), color }).collect();
            }
            sheet.tables = tables.remove(&s.id).unwrap_or_default();
            // the images of its title block
            if let Some((t, kind)) = s.title_block.as_ref().and_then(|t| Some((t, block_kind(t)?))) {
                // decoded here: an image that cannot be read is left out, the others show at once
                let images = super::rvt_images::images(doc, kind).into_iter().filter_map(|i| {
                    let pixels = image::load_from_memory(&i.file).ok()?.to_rgba8();
                    Some(crate::model::SheetImage {
                        center: [(t.origin[0] * 0.3048 + i.center[0]) as f32, (t.origin[1] * 0.3048 + i.center[1]) as f32],
                        size: i.size.map(|x| x as f32),
                        angle: i.angle as f32,
                        image: iced::widget::image::Handle::from_rgba(pixels.width(), pixels.height(), pixels.into_raw()),
                    })
                });
                sheet.images = images.collect();
            }
            sheet
        })
        .collect()
}

#[cfg(test)]
mod tests {
    /// A project with drawings (`RVT_DRAWINGS`, read-only; skipped when absent): its plans and
    /// sections become views of the document, with the texts and dimensions drawn in them.
    #[test]
    fn views_become_drawings_of_the_document() {
        let Some(path) = std::env::var_os("RVT_DRAWINGS").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_DRAWINGS to a Revit project with annotated views");
            return;
        };
        let t = std::time::Instant::now();
        let model = crate::loaders::load(&path).expect("rvt");
        eprintln!("loaded in {:?}, {} drawing nodes", t.elapsed(), model.project.drawings.len());
        let document = crate::document::Document::from_model(&model);
        let (plans, sections) = (document.plans(), document.sections());
        eprintln!("{} plans, {} sections and elevations", plans.len(), sections.len());
        for p in plans {
            eprintln!("plan {:?} {:?} cut {:.2} depth {:.2} crop {:?}: {} notes, {} dimensions", p.name, p.kind, p.cut, p.depth, p.crop, p.notes.len(), p.dimensions.len());
            for n in p.notes.iter().take(3) {
                eprintln!("    note {:?} {:?}", n.at, n.text);
            }
            for d in p.dimensions.iter().take(3) {
                eprintln!("    dimension {:?} -> {:?} = {:.0} mm", d.from, d.to, d.length() * 1000.0);
            }
        }
        for s in sections {
            eprintln!("section {:?} {:?} {:?} -> {:?} depth {:.2} crop {:?}: {} notes, {} dimensions", s.name, s.kind, s.from, s.to, s.depth, s.crop, s.notes.len(), s.dimensions.len());
            for n in s.notes.iter().take(3) {
                eprintln!("    note {:?} {:?}", n.at, n.text);
            }
            for d in s.dimensions.iter().take(3) {
                eprintln!("    dimension {:?} -> {:?} = {:.0} mm", d.from, d.to, d.length() * 1000.0);
            }
        }
        assert!(!plans.is_empty() && !sections.is_empty());
        // texts and lines: a dimension of the file is drawn as the lines and texts it has there
        assert!(plans.iter().any(|p| !p.notes.is_empty() && !p.lines.is_empty()) || sections.iter().any(|s| !s.notes.is_empty() && !s.lines.is_empty()));
        // marks lie in or near the model: a wrong frame would put them far away
        let b = &model.bounds;
        let near = |p: [f64; 2]| p[0] > b.min[0] as f64 - 50.0 && p[0] < b.max[0] as f64 + 50.0 && p[1] > b.min[1] as f64 - 50.0 && p[1] < b.max[1] as f64 + 50.0;
        assert!(plans.iter().flat_map(|p| &p.notes).all(|n| near(n.at)), "bounds {b:?}");
    }

    use super::*;

    fn wall(start: glam::DVec2, end: glam::DVec2) -> SourceWall {
        SourceWall { start, end, base: 1.0, height: 3.0, thickness: 0.3, openings: Vec::new(), level: None }
    }

    /// A box in feet from metres.
    fn feet(b: [f64; 6]) -> [f64; 6] {
        b.map(|v| v / FT)
    }

    /// A door or window known by its box (metres) alone.
    fn boxed(b: [f64; 6]) -> Hole {
        Hole { bb: Some(feet(b)), size: None, origin: None, bottom: None }
    }

    fn opening(cut: Cut) -> bim_core::Opening {
        match cut {
            Cut::Opening(o) => o,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_door_in_a_wall_along_x_opens_where_its_box_is() {
        let w = wall(glam::DVec2::ZERO, glam::DVec2::new(6.0, 0.0));
        // 0.9 wide from 2 m, 2.1 high from the wall's base, its frame standing out of the wall
        let o = opening(cut_of(&w, &boxed([2.0, -0.2, 1.0, 2.9, 0.2, 3.1])));
        assert!((o.along - 2.0).abs() < 1e-9 && (o.width - 0.9).abs() < 1e-9 && o.sill.abs() < 1e-9 && (o.height - 2.1).abs() < 1e-9, "{o:?}");
        // drawn the other way, along y: the same opening, measured from that wall's start
        let w = wall(glam::DVec2::new(0.0, 6.0), glam::DVec2::ZERO);
        let o = opening(cut_of(&w, &boxed([-0.2, 2.0, 2.0, 0.2, 2.9, 3.2])));
        assert!((o.along - 3.1).abs() < 1e-9 && (o.width - 0.9).abs() < 1e-9 && (o.sill - 1.0).abs() < 1e-9 && (o.height - 1.2).abs() < 1e-9, "{o:?}");
    }

    #[test]
    fn a_door_with_its_types_sizes_opens_about_its_insertion_point() {
        let w = wall(glam::DVec2::ZERO, glam::DVec2::new(6.0, 0.0));
        // a box that reaches far to one side (its swing) and above the wall; the type says 0.9 × 2.1
        let hole = Hole { bb: Some(feet([1.5, -0.2, 0.9, 3.4, 0.2, 4.1])), size: Some((0.9 / FT, 2.1 / FT)), origin: Some([3.0 / FT, 0.0, 1.0 / FT]), bottom: Some(1.0 / FT) };
        let o = opening(cut_of(&w, &hole));
        assert!((o.along - 2.55).abs() < 1e-9 && (o.width - 0.9).abs() < 1e-9 && o.sill.abs() < 1e-9 && (o.height - 2.1).abs() < 1e-9, "{o:?}");
        // a type without sizes: its box
        let o = opening(cut_of(&w, &Hole { size: None, ..hole }));
        assert!((o.along - 1.5).abs() < 1e-9 && (o.width - 1.9).abs() < 1e-9 && (o.height - 3.0).abs() < 1e-9, "{o:?}");
    }

    #[test]
    fn a_swing_drawn_in_plan_cuts_nothing_and_a_door_without_a_box_or_type_is_unknown() {
        let w = wall(glam::DVec2::ZERO, glam::DVec2::new(6.0, 0.0));
        assert_eq!(cut_of(&w, &boxed([2.0, -0.2, 1.0, 2.9, 0.2, 1.0])), Cut::Nothing, "no height");
        assert_eq!(cut_of(&w, &boxed([7.0, -0.2, 1.0, 8.0, 0.2, 3.0])), Cut::Nothing, "beside the wall");
        assert_eq!(cut_of(&w, &Hole { bb: None, size: None, origin: None, bottom: None }), Cut::Unknown);
    }

    #[test]
    fn the_parts_of_one_door_make_one_opening_and_apart_ones_stay_apart() {
        let o = |along: f64, sill: f64, width: f64, height: f64| bim_core::Opening { along, sill, width, height };
        // frame, leaf and lining of one door, overlapping; a window beside it
        let got = merged(vec![o(1.7, 0.07, 1.58, 2.45), o(4.0, 0.9, 1.0, 1.2), o(1.78, 0.14, 1.44, 2.39), o(1.7, 0.07, 1.58, 2.44)]);
        assert_eq!(got.len(), 2);
        assert!((got[0].along - 1.7).abs() < 1e-12 && (got[0].width - 1.58).abs() < 1e-12 && (got[0].height - 2.46).abs() < 1e-12, "{got:?}");
        assert_eq!(got[1], o(4.0, 0.9, 1.0, 1.2));
    }

    #[test]
    fn a_window_in_a_turned_wall_takes_its_width_from_its_box_unless_the_wall_is_near_45_degrees() {
        // a wall at 30°, a window 1.2 along it and 0.3 across, centred 2 m from the start
        let d = 30_f64.to_radians();
        let dir = glam::DVec2::new(d.cos(), d.sin());
        let w = wall(glam::DVec2::ZERO, dir * 5.0);
        let centre = dir * 2.0;
        let (c, s) = (d.cos(), d.sin());
        let (ex, ey) = (1.2 * c + 0.3 * s, 1.2 * s + 0.3 * c);
        let bb = [centre.x - ex / 2.0, centre.y - ey / 2.0, 1.9, centre.x + ex / 2.0, centre.y + ey / 2.0, 3.1];
        let o = opening(cut_of(&w, &boxed(bb)));
        assert!((o.width - 1.2).abs() < 1e-9 && (o.along - 1.4).abs() < 1e-9 && (o.sill - 0.9).abs() < 1e-9, "{o:?}");
        // at 45° the box does not tell
        let dir = glam::DVec2::new(1.0, 1.0).normalize();
        let w = wall(glam::DVec2::ZERO, dir * 5.0);
        assert_eq!(cut_of(&w, &boxed([1.0, 1.0, 1.0, 2.0, 2.0, 3.0])), Cut::Unknown);
    }

    /// The location line of a sample's walls against their meshes: how far the middle of each
    /// wall's mesh lies from its line, across it, for the walls the file draws with their ids.
    /// Set `RVT_SAMPLE` to a Revit 2024+ file; skipped when absent.
    #[test]
    fn walls_of_a_sample_lie_on_their_meshes() {
        let Some(path) = std::env::var_os("RVT_SAMPLE").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_SAMPLE to a Revit 2024+ file");
            return;
        };
        let model = crate::loaders::load(&path).expect("rvt");
        let walls = &model.project.walls;
        let (mut centred, mut off, mut thick, mut drawn) = (0, 0, 0, 0);
        for (&id, w) in walls {
            let ranges = model.element_ranges(id);
            if ranges.is_empty() {
                continue;
            }
            drawn += 1;
            let Some((lo, hi)) = crate::app::across_mesh(&model, id, w.start, w.end) else { continue };
            if ((hi - lo) - w.thickness).abs() > 0.005 {
                thick += 1;
                continue;
            }
            if ((lo + hi) / 2.0).abs() <= 0.001 {
                centred += 1;
            } else {
                off += 1;
            }
        }
        eprintln!("{} walls, {drawn} drawn with their ids: {centred} centred on their line, {off} beside it, {thick} not as thick as their type", walls.len());
        assert!(!walls.is_empty());
        // what reading them adds to a load: the mesher alone, and with the walls read beside it
        // (each on a document of its own, so neither finds the other's caches warm)
        let doc = rvt_read::Document::open(&path).expect("rvt");
        let t = std::time::Instant::now();
        drop(rvt_export::mesh::scene(&doc));
        let alone = t.elapsed();
        let doc = rvt_read::Document::open(&path).expect("rvt");
        let t = std::time::Instant::now();
        let again = std::thread::scope(|s| {
            let walls = s.spawn(|| source_walls(&doc));
            drop(rvt_export::mesh::scene(&doc));
            walls.join().unwrap_or_default()
        });
        eprintln!("mesher {alone:?} alone, {:?} with the walls read beside it; {} walls with openings", t.elapsed(), again.values().filter(|w| !w.openings.is_empty()).count());
    }
    /// Autodesk's racbasicsampleproject_2026.rvt, read-only from `RVT_SAMPLE`; skipped when absent.
    #[test]
    fn loads_revit_sample() {
        let Some(path) = std::env::var_os("RVT_SAMPLE").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_SAMPLE to racbasicsampleproject_2026.rvt");
            return;
        };
        let model = crate::loaders::load(&path).expect("rvt");
        assert!(model.layers.len() > 3, "{} layers", model.layers.len());
        assert!(model.triangle_count() > 1000);
        // Z up: a building is wider than it is tall
        let s = model.bounds.size();
        assert!(s[2] < s[0].max(s[1]), "size {s:?}");
        // material colours, pickable elements and their properties
        assert!(model.layers.iter().any(|l| l.colors.is_some()));
        let elements: usize = model.layers.iter().map(|l| l.elements.len()).sum();
        assert!(elements > 100 && model.props.len() > 100, "{elements} elements, {} props", model.props.len());
        let c = model.bounds.center();
        let t = std::time::Instant::now();
        let hit = crate::app::pick_triangle(&model, &vec![true; model.layers.len()], glam::Vec3::new(c[0], c[1], model.bounds.max[2] + 10.0), glam::Vec3::new(0.0, 0.0, -1.0), &|_, _| glam::Vec3::ZERO, &|_| true, &|_, _| false, None);
        let id = hit.and_then(|(l, t)| model.layers[l].element_at(t));
        eprintln!("pick {:?} in {:?}: {:?}", id, t.elapsed(), id.map(|id| model.element_props(id)));
        // material transparency: alpha per vertex
        let mut alphas = std::collections::BTreeMap::<u8, usize>::new();
        for c in model.layers.iter().filter_map(|l| l.colors.as_ref()).flatten() {
            *alphas.entry(c[3]).or_default() += 1;
        }
        eprintln!("alpha histogram {alphas:?}");
        // project data: racbasicsample has levels and phases
        let pd = &model.project;
        eprintln!("project info {:?}\nlevels {:?}\nphases {:?}\ngrids {}\nrooms {}", pd.info, pd.levels, pd.phases, pd.grids.len(), pd.rooms.len());
        for r in pd.rooms.iter().take(5) {
            eprintln!("room {:?}", r);
        }
        assert!(pd.levels.len() >= 2 && !pd.phases.is_empty());
        // the views (beta): racbasicsample has 3D views, plans, sections and elevations
        assert!(model.views.iter().any(|v| v.kind == crate::model::ViewKind::ThreeD && v.camera.is_some()), "{:?}", model.views);
        assert!(model.views.iter().any(|v| v.plan.is_some()));
        for v in &model.views {
            eprintln!("view {:?} {:?} camera {} clip {} plan {:?}", v.kind, v.name, v.camera.is_some(), v.clip.is_some(), v.plan);
        }
        eprintln!(
            "rvt: {} layers, {} triangles, {:?}, bounds {:?}",
            model.layers.len(),
            model.triangle_count(),
            model.load_time,
            model.bounds
        );
    }
}
