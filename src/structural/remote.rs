//! The MCP tools on structural analysis models: `list_structural`, `get_structural_issues`,
//! `get_structural_element` and `find_structural`. Each is a function of the open model and the
//! tool's arguments; `App::command` (`src/remote.rs`) calls them.

use super::{has_unit, kind_from_name, kind_name, preset_family, StructuralItem};
use crate::model::{Bounds, Model};
use crate::viewport::render::transform_bounds;
use bim_structural::{support, Curve, ItemKind, ModelKind, Target};
use glam::{DVec3, Mat4, Vec3};
use serde_json::{json, Map, Value};

type Answer = Result<Value, String>;

/// f64 metres to 1e-9 (sub-micrometre), as JSON.
fn m(v: f64) -> Value {
    json!((v * 1e9).round() / 1e9)
}

fn v3(v: DVec3) -> Value {
    json!([m(v.x), m(v.y), m(v.z)])
}

fn f32v(v: [f32; 3]) -> Value {
    json!(v.map(|x| (x as f64 * 1e4).round() / 1e4))
}

fn jbounds(b: &Bounds) -> Value {
    let (min, max) = (Vec3::from_array(b.min), Vec3::from_array(b.max));
    json!({ "min": f32v(b.min), "max": f32v(b.max), "size": f32v((max - min).to_array()), "center": f32v(b.center()) })
}

fn props_value(props: Vec<(String, String)>) -> Value {
    Value::Object(props.into_iter().map(|(k, v)| (k, Value::String(v))).collect())
}

/// The parts a `model` argument names: a part index, or (part of) an analysis model's name; all
/// parts when absent.
fn parts_of(model: &Model, a: &Value) -> Result<Vec<usize>, String> {
    let all: Vec<usize> = (0..model.structural.len()).collect();
    match a.get("model") {
        None | Some(Value::Null) => Ok(all),
        Some(Value::Number(n)) => {
            let i = n.as_u64().ok_or("model: a part index or a name")? as usize;
            if i < model.structural.len() { Ok(vec![i]) } else { Err(format!("no structural model {i}: there are {}", model.structural.len())) }
        }
        Some(Value::String(s)) => {
            let s = s.to_lowercase();
            let found: Vec<usize> = all.into_iter().filter(|&i| model.structural[i].doc.name.to_lowercase().contains(&s)).collect();
            if found.is_empty() { Err(format!("no structural model named '{s}'")) } else { Ok(found) }
        }
        Some(_) => Err("model: a part index or a name".into()),
    }
}

/// `list_structural`: one entry per analysis model with its source, origin and counts.
pub fn list(model: &Model) -> Answer {
    let parts: Vec<Value> = model
        .structural
        .iter()
        .enumerate()
        .map(|(i, part)| {
            let d = &part.doc;
            let origin = model.origins.get(part.model as usize).copied().unwrap_or_default();
            json!({
                "part": i,
                "model": part.model,
                "name": d.name,
                "kind": match d.kind { ModelKind::Analytical => "analytical", ModelKind::PhysicalAxes => "physical_axes" },
                "source": { "format": d.source.format, "file": d.source.file, "application": d.source.application },
                "origin": origin.anchor.map(v3),
                "aligned_by": v3(origin.shift),
                "counts": {
                    "nodes": d.nodes.len(),
                    "members": d.members.len(),
                    "surfaces": d.surfaces.len(),
                    "line_supports": d.line_supports.len(),
                    "sections": d.sections.len(),
                    "materials": d.materials.len(),
                    "load_cases": d.load_cases.len(),
                    "combinations": d.combinations.len(),
                    "actions": d.actions.len(),
                    "result_sets": d.results.len(),
                    "supports": d.nodes.iter().filter(|n| n.support.is_some()).count(),
                },
                "diagnostics": d.diagnostics.len(),
            })
        })
        .collect();
    Ok(json!({
        "parts": parts,
        "units": "SI: metres, newtons, radians; coordinates are world coordinates of the source file",
    }))
}

/// `get_structural_issues`: the diagnostics of one or all analysis models.
pub fn issues(model: &Model, a: &Value) -> Answer {
    let mut out = Vec::new();
    for i in parts_of(model, a)? {
        let part = &model.structural[i];
        for d in &part.doc.diagnostics {
            out.push(json!({
                "part": i,
                "model": part.doc.name,
                "object": d.object.map(|id| part.element_id(id)),
                "source_object": d.object,
                "code": d.code,
                "message": d.message,
            }));
        }
    }
    Ok(json!({ "total": out.len(), "issues": out }))
}

/// `get_structural_element`: one object with its coordinates (f64 world metres), frame, section,
/// insertion, releases and support, as the document holds them.
pub fn element(model: &Model, a: &Value, mm: &Mat4) -> Answer {
    let id = a.get("id").and_then(|v| v.as_u64().or_else(|| v.as_str()?.trim().parse().ok())).ok_or("id is required")?;
    let item = model.structural_item(id).ok_or_else(|| format!("no structural object {id}"))?;
    Ok(element_json(model, &item, mm))
}

fn element_json(model: &Model, item: &StructuralItem, mm: &Mat4) -> Value {
    let d = item.doc();
    let i = item.item.slot as usize;
    let origin = model.origins.get(item.part.model as usize).copied().unwrap_or_default();
    let mut out = json!({
        "id": item.element,
        "kind": kind_name(item.kind()),
        "number": item.number(),
        "part": item.part_index,
        "model": item.part.model,
        "analysis_model": d.name,
        "source_id": item.source,
        "unit": item.unit_key(),
        "properties": props_value(item.props()),
        "bounds": item.bounds(&model.origins).map(|b| jbounds(&transform_bounds(&b, mm))),
    });
    let node_json = |id: u64| {
        d.node(id).map(|n| json!({ "id": item.part.element_id(id), "number": n.number, "world": v3(n.p), "display": f32v(origin.display(n.p).as_vec3().to_array()) }))
    };
    match item.kind() {
        ItemKind::Node => {
            let n = &d.nodes[i];
            out["world"] = v3(n.p);
            out["display"] = f32v(origin.display(n.p).as_vec3().to_array());
            if let Some(f) = n.frame {
                out["frame"] = json!([v3(f.x_axis), v3(f.y_axis), v3(f.z_axis)]);
            }
            if let Some(s) = &n.support {
                out["support"] = json!({
                    "dofs": support::code(&s.dofs),
                    "preset": preset_family(s.preset),
                    "named": s.named,
                    "frame": format!("{:?}", s.frame),
                });
            }
        }
        ItemKind::Member => {
            let mem = &d.members[i];
            out["nodes"] = Value::Array(mem.nodes.iter().filter_map(|&n| node_json(n)).collect());
            if let Some((a, b)) = d.member_ends(mem) {
                out["length"] = m(bim_structural::frame::curve_length(a, b, &mem.geometry));
                if let Ok(f) = bim_structural::frame::member_frame(a, b, mem.z_ref) {
                    let r = f.rotation();
                    out["frame"] = json!({ "x": v3(r.x_axis), "y": v3(r.y_axis), "z": v3(r.z_axis) });
                }
            }
            out["geometry"] = match &mem.geometry {
                Curve::Line => json!("line"),
                Curve::Arc { mid } => json!({ "arc": { "mid": v3(*mid) } }),
                Curve::Polyline(p) => json!({ "polyline": p.iter().map(|x| v3(*x)).collect::<Vec<_>>() }),
            };
            out["section"] = mem.section.map_or(Value::Null, |(s, e)| {
                let sec = |id: u64| d.section(id).map(|s| json!({ "id": item.part.element_id(id), "name": s.name }));
                json!({ "start": sec(s), "end": e.and_then(sec) })
            });
            let ins = &mem.insertion;
            out["insertion"] = json!({
                "cardinal_point": ins.cardinal_point,
                "eccentricity": [m(ins.eccentricity.x), m(ins.eccentricity.y)],
                "end_offsets": { "start": v3(ins.end_offsets[0]), "end": v3(ins.end_offsets[1]), "frame": format!("{:?}", ins.end_offset_frame) },
                "profile_position": [m(ins.profile_position.translation.x), m(ins.profile_position.translation.y)],
                "profile_offsets": ins.profile_offsets.map(|[a, b]| json!([[m(a.x), m(a.y)], [m(b.x), m(b.y)]])),
                "policy": format!("{:?}", d.source.eccentricity_policy),
                "notes": "offsets are recorded as the source gives them; solids apply them from phase S6",
            });
            out["releases"] = json!(mem.releases.iter().map(|r| r.as_ref().map(|h| json!({ "dofs": support::code(&h.dofs), "named": h.named }))).collect::<Vec<_>>());
        }
        ItemKind::Surface => {
            let s = &d.surfaces[i];
            out["boundary"] = Value::Array(s.boundary.iter().filter_map(|&n| node_json(n)).collect());
        }
        ItemKind::Action => {
            let act = &d.actions[i];
            out["target"] = match &act.target {
                Target::Node(id) | Target::Surface(id) | Target::Member { id, .. } => json!(item.part.element_id(*id)),
                Target::Free(p) => json!(p.iter().map(|x| v3(*x)).collect::<Vec<_>>()),
            };
        }
        _ => {}
    }
    out["note"] = json!("coordinates: 'world' in f64 metres of the source file, 'display' and 'bounds' in the viewer's scene (as get_element)");
    out
}

/// `find_structural`: ids of the objects of one kind that pass the filters, a page at a time.
/// Filters: `support` (fixed, pinned, roller, sliding, spring, free, custom, or any), `section`
/// (name contains), `member_kind`, `number` (exact) or `number_from`/`number_to` (numeric range),
/// `model`, and `within` (a box in scene coordinates, as get_element's bounds; units only).
pub fn find(model: &Model, a: &Value, mm: &Mat4) -> Answer {
    let kind = match a.get("kind").and_then(Value::as_str) {
        Some(k) => Some(kind_from_name(k).ok_or_else(|| format!("kind '{k}': node, member, surface, line_support, section, material, load_case, combination, action or result_set"))?),
        None => None,
    };
    let parts = parts_of(model, a)?;
    let lower = |key: &str| a.get(key).and_then(Value::as_str).map(str::to_lowercase);
    let support_filter = lower("support");
    let section_filter = lower("section");
    let member_kind = lower("member_kind");
    let number = a.get("number").map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string));
    let from = a.get("number_from").and_then(Value::as_f64);
    let to = a.get("number_to").and_then(Value::as_f64);
    let within = match a.get("within") {
        None | Some(Value::Null) => None,
        Some(w) => {
            let corner = |k: &str| {
                w.get(k).and_then(Value::as_array).filter(|c| c.len() == 3).and_then(|c| {
                    Some([c[0].as_f64()? as f32, c[1].as_f64()? as f32, c[2].as_f64()? as f32])
                })
            };
            let (Some(min), Some(max)) = (corner("min"), corner("max")) else {
                return Err("within: { \"min\": [x, y, z], \"max\": [x, y, z] }".into());
            };
            Some((Bounds { min, max }, w.get("crossing").and_then(Value::as_bool).unwrap_or(false)))
        }
    };
    let (offset, limit) = (
        a.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize,
        a.get("limit").and_then(Value::as_u64).unwrap_or(1000).min(10_000) as usize,
    );

    let mut ids = Vec::new();
    for item in model.structural_index.all(&model.structural) {
        if !parts.contains(&(item.part_index as usize)) || kind.is_some_and(|k| k != item.kind()) {
            continue;
        }
        let d = item.doc();
        let i = item.item.slot as usize;
        if let Some(want) = &support_filter {
            let ok = match item.kind() {
                ItemKind::Node => d.nodes[i].support.as_ref().is_some_and(|s| {
                    want == "any" || preset_family(s.preset) == want || s.named.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(want))
                }),
                _ => false,
            };
            if !ok {
                continue;
            }
        }
        if let Some(want) = &section_filter {
            let ok = match item.kind() {
                ItemKind::Member => d.members[i].section.is_some_and(|(s, e)| {
                    [Some(s), e].into_iter().flatten().any(|id| d.section(id).is_some_and(|x| x.name.to_lowercase().contains(want.as_str())))
                }),
                ItemKind::Section => d.sections[i].name.to_lowercase().contains(want.as_str()),
                _ => false,
            };
            if !ok {
                continue;
            }
        }
        if let Some(want) = &member_kind {
            if item.kind() != ItemKind::Member || !format!("{:?}", d.members[i].kind).eq_ignore_ascii_case(&want.replace('_', "")) {
                continue;
            }
        }
        let no = item.number();
        if number.as_ref().is_some_and(|n| *n != no) {
            continue;
        }
        if from.is_some() || to.is_some() {
            let Ok(x) = no.trim().parse::<f64>() else { continue };
            if from.is_some_and(|f| x < f) || to.is_some_and(|t| x > t) {
                continue;
            }
        }
        if let Some((b, crossing)) = &within {
            if !has_unit(item.kind()) {
                continue;
            }
            let Some(ub) = item.bounds(&model.origins).map(|u| transform_bounds(&u, mm)) else { continue };
            let inside = (0..3).all(|k| ub.min[k] >= b.min[k] && ub.max[k] <= b.max[k]);
            let touches = (0..3).all(|k| ub.max[k] >= b.min[k] && ub.min[k] <= b.max[k]);
            if !(if *crossing { touches } else { inside }) {
                continue;
            }
        }
        ids.push(item.element);
    }
    let total = ids.len();
    let page: Vec<u64> = ids.into_iter().skip(offset).take(limit).collect();
    let mut out = Map::new();
    out.insert("total".into(), json!(total));
    out.insert("offset".into(), json!(offset));
    out.insert("returned".into(), json!(page.len()));
    out.insert("truncated".into(), json!(offset + page.len() < total));
    out.insert("ids".into(), json!(page));
    Ok(Value::Object(out))
}

/// A structural object as `get_element` shows it (no triangles: bounds from the document).
pub fn as_element(model: &Model, id: u64, mm: &Mat4) -> Option<Value> {
    let item = model.structural_item(id)?;
    Some(json!({
        "id": id,
        "layers": [],
        "structural": { "kind": kind_name(item.kind()), "number": item.number(), "part": item.part_index, "model": item.part.model, "source_id": item.source },
        "properties": props_value(item.props()),
        "triangles": 0,
        "bounds": item.bounds(&model.origins).map(|b| jbounds(&transform_bounds(&b, mm))),
    }))
}
