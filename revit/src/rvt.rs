//! Revit (.rvt) via the OpenAEC rvt reader: one layer per category batch, with material colours
//! per vertex, the drawn elements (for picking) and their properties.

use open_3d_viewer::loaders::{Loaded, Loader, Phase, Progress, Source};
use open_3d_viewer::model::{
    y_up_to_z_up, GridLine, Later, Layer, PlanRange, ProjectData, Room, SavedView, Schedule, ScheduleRow, ViewCamera, ViewKind,
};
use rvt_model::{FieldValue, ObjectView};
use std::collections::HashMap;
use open_3d_viewer::section::SectionBox;
use glam::Vec3;

pub struct RvtLoader;

impl Loader for RvtLoader {
    fn name(&self) -> &'static str {
        "Revit"
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
        // `scene` returns view coordinates: metres, Y up (x, z, -y)
        let scene = rvt_export::mesh::scene(&doc);
        let views = saved_views(&doc);
        let mut project = project_data(&doc, &scene.levels);
        // the schedules take seconds on a large project: computed once the model shows
        project.schedules = Later::new(move || schedules(&doc));
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
                }
            })
            .collect();
        Ok(Loaded { layers, views, project })
    }
}

/// The schedules as the viewer shows them: templates left out, the shown columns only.
fn schedules(doc: &rvt_read::Document) -> Vec<Schedule> {
    rvt_export::schedules::schedules(doc)
        .into_iter()
        .filter(|s| !s.is_template)
        .map(|s| {
            let shown: Vec<usize> = (0..s.columns.len()).filter(|&i| !s.columns[i].hidden).collect();
            Schedule {
                name: s.name,
                title: s.title,
                columns: shown.iter().map(|&i| s.columns[i].heading.clone()).collect(),
                rows: s
                    .rows
                    .into_iter()
                    .map(|r| ScheduleRow {
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
fn saved_views(doc: &rvt_read::Document) -> Vec<SavedView> {
    use rvt_export::views::{OrientedBox, ViewKind as K};
    const FT: f32 = 0.3048;
    let v3 = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let frame = |b: &OrientedBox| SectionBox::from_frame(v3(b.origin) * FT, b.basis.map(v3), v3(b.min) * FT, v3(b.max) * FT);
    // ±1e9 ft means unlimited
    let limited = |v: Option<f64>| v.filter(|v| v.abs() < 1e8);
    rvt_export::views::views(doc)
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
            let mut out = SavedView { name: v.name.clone(), kind, camera: None, clip: None, plan: None, level: v.level_name.clone() };
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
                    let (low, high) = if ceiling {
                        (cut, limited(range.and_then(|r| r.top)).unwrap_or(cut + 10.0))
                    } else {
                        // down to the view depth, so the floor below the cut shows
                        let low = limited(range.and_then(|r| r.depth)).or(limited(range.and_then(|r| r.bottom))).or(level).unwrap_or(cut - 10.0);
                        (low - 0.01, cut)
                    };
                    out.plan = Some(PlanRange { low: low as f32 * FT, high: high as f32 * FT, from_below: ceiling });
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
            Some(out)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    /// Autodesk's racbasicsampleproject_2026.rvt, read-only from `RVT_SAMPLE`; skipped when absent.
    #[test]
    fn loads_revit_sample() {
        let Some(path) = std::env::var_os("RVT_SAMPLE").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_SAMPLE to racbasicsampleproject_2026.rvt");
            return;
        };
        open_3d_viewer::loaders::add(vec![&super::RvtLoader]);
        let model = open_3d_viewer::loaders::load(&path).expect("rvt");
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
        let hit = open_3d_viewer::app::pick_triangle(&model, &vec![true; model.layers.len()], glam::Vec3::new(c[0], c[1], model.bounds.max[2] + 10.0), glam::Vec3::new(0.0, 0.0, -1.0), &|_, _| glam::Vec3::ZERO, &|_| true);
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
        assert!(model.views.iter().any(|v| v.kind == open_3d_viewer::model::ViewKind::ThreeD && v.camera.is_some()), "{:?}", model.views);
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
