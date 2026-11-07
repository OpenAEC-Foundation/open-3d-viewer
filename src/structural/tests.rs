//! Phase S2 exit tests that run headless: analysis-only files open and fit to view, structural
//! objects are elements (`get_element`, `find_elements`), combining keeps every structural id and
//! both origins and aligns a 5 km offset within 1 mm, box selection finds nodes and members, and
//! the MCP tools answer.

use super::*;
use crate::app::App;
use crate::explode::ExplodeIndex;
use crate::loaders::{self, Progress, Source};
use crate::model::{Layer, Model, ALIGN_LIMIT};
use crate::remote::RemoteCall;
use bim_structural::{sam_id, IdKind, Node, SourceInfo};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/bim-structural/tests/fixtures").join(name)
}

fn run(app: &mut App, cmd: &str, args: Value) -> Result<Value, String> {
    let (call, rx) = RemoteCall::new(cmd, args);
    let _ = app.remote(call);
    rx.try_recv().expect("answered at once")
}

fn app_with(model: Model) -> App {
    let mut app = App::default();
    app.set_model(Arc::new(model));
    app
}

const N1: u64 = 72057594037927937; // sam_id(Node, 1)

/// The element id of document id `source` in the first part of `model`.
fn el(model: &Model, source: u64) -> u64 {
    model.structural[0].element_of(source).expect("an object of the first part")
}

/// Every id in an MCP answer (`id`, `ids`, `object` and `unit` keys) survives a reader that takes
/// JSON numbers as f64, as MCP clients in JavaScript do: a number at most 2^53, or a string.
fn assert_ids_survive_f64(v: &Value) {
    match v {
        Value::Object(map) => {
            for (k, x) in map {
                if matches!(k.as_str(), "id" | "ids" | "object" | "unit") {
                    for id in x.as_array().cloned().unwrap_or_else(|| vec![x.clone()]) {
                        if let Some(n) = id.as_u64() {
                            assert_eq!((n as f64) as u64, n, "{k}: {n} does not survive an f64");
                            assert!(n <= MAX_SAFE_ID, "{k}: {n}");
                        }
                    }
                }
                assert_ids_survive_f64(x);
            }
        }
        Value::Array(list) => list.iter().for_each(assert_ids_survive_f64),
        _ => {}
    }
}

#[test]
fn nodes_only_samjson_opens_and_fits_to_view() {
    let model = loaders::load(&fixture("nodes_only.samjson")).expect("a nodes-only file opens");
    assert_eq!(model.format, "Structural analysis model (sam-json)");
    assert!(model.layers.is_empty());
    assert_eq!(model.structural.len(), 1);
    assert_eq!(model.structural_index.len(), 4);
    // the anchor is subtracted for display, kept in f64 for the world
    assert_eq!(model.origins[0].anchor, Some(DVec3::new(155000.0, 463000.0, 0.0)));
    assert_eq!(model.bounds.min, [0.0, 0.0, 0.0]);
    assert_eq!(model.bounds.max, [6.0, 4.0, 3.5]);
    // diagnostics are model notes
    assert!(model.project.info.iter().any(|(k, v)| k == "Model notes" && v == "1"));
    assert!(model.project.info.iter().any(|(k, v)| k == "Note 1" && v.starts_with("named-support-expanded") && v.ends_with("(node 2)")));
    // compact element ids, at or below 2^53, back to their source ids
    assert_eq!(el(&model, N1), 1);
    assert!(model.structural_index.element_ids().all(|id| id <= MAX_SAFE_ID));

    // the browser path: bytes, not a file
    let data = std::fs::read(fixture("nodes_only.samjson")).unwrap();
    let source = Source::Bytes { name: "nodes.SAMJSON".into(), data: Arc::new(data) };
    let bytes = loaders::load_with(&source, &Progress::default()).expect("opens from bytes");
    assert_eq!(bytes.bounds, model.bounds);

    // fit to view: the camera frames the nodes
    let app = app_with(model);
    assert_eq!(app.camera.target, glam::Vec3::new(3.0, 2.0, 1.75));
    assert!(app.camera.distance > 3.0);
    let explode = app.explode_index.as_ref().unwrap();
    assert_eq!(explode.len(), 5, "unit 0 and one unit per node");
    assert_eq!(explode.unit_bounds(explode.structural_unit(1).unwrap()).unwrap().min, [0.0; 3]);
}

#[test]
fn the_portal_frame_opens_with_its_members() {
    let model = loaders::load(&fixture("portal_frame.samjson")).expect("portal frame");
    let d = &model.structural[0].doc;
    assert_eq!((d.nodes.len(), d.members.len()), (4, 3));
    assert_eq!(model.bounds.size(), [6.0, 0.0, 4.0]);
    let member = el(&model, sam_id(IdKind::Member, 1));
    let item = model.structural_item(member).unwrap();
    assert_eq!((item.kind(), item.source), (ItemKind::Member, sam_id(IdKind::Member, 1)));
    assert_eq!(item.unit_key(), Some(member));
    let props = model.element_props(member);
    let get = |k: &str| props.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
    assert_eq!(get("Class").as_deref(), Some("Structural member"));
    assert_eq!(get("Section").as_deref(), Some("HEB 200"));
    assert_eq!(get("Length").as_deref(), Some("4.000 m"));
    assert_eq!(get("Cardinal point").as_deref(), Some("8"));
    assert_eq!(model.element_bounds(member).unwrap().max, [0.0, 0.0, 4.0]);
    // a load sits on its member (the beam): its unit is the member's
    let action = el(&model, model.structural[0].doc.actions[0].id);
    let beam = el(&model, sam_id(IdKind::Member, 2));
    assert_eq!(model.structural_item(action).unwrap().unit_key(), Some(beam));
    assert_eq!(model.element_bounds(action), model.element_bounds(beam));
}

#[test]
fn files_of_only_sections_open_and_empty_files_fail() {
    // a section library: no nodes, but objects the MCP tools reach
    let text = r#"{ "format": "sam-json", "version": 1, "source": {}, "units": "SI", "models": [ { "id": 1, "name": "Library",
        "materials": [ { "id": 1, "name": "S235", "E": 2.1e11 } ],
        "sections": [ { "id": 1, "name": "HEB 200", "material": 1, "shape": { "I": { "h": 0.2, "b": 0.2, "tw": 0.009, "tf": 0.015, "r": 0.018, "slope": 0 } } } ] } ] }"#;
    let source = Source::Bytes { name: "library.samjson".into(), data: Arc::new(text.as_bytes().to_vec()) };
    let model = loaders::load_with(&source, &Progress::default()).expect("a library opens");
    assert_eq!(model.structural_index.len(), 2);
    assert!(model.bounds.is_valid(), "a box around the origin to frame");
    let mut app = app_with(model);
    let found = run(&mut app, "find_structural", json!({ "kind": "section" })).unwrap();
    assert_eq!(found["total"], json!(1));
    let id = found["ids"][0].clone();
    assert_eq!(run(&mut app, "get_structural_element", json!({ "id": id })).unwrap()["number"], json!("HEB 200"));
    assert!(app.camera.distance.is_finite());

    // an analysis model without any object, and no triangles: nothing to show

    let text = r#"{ "format": "sam-json", "version": 1, "source": {}, "units": "SI", "models": [ { "id": 1, "name": "Empty" } ] }"#;
    let source = Source::Bytes { name: "empty.samjson".into(), data: Arc::new(text.as_bytes().to_vec()) };
    let error = loaders::load_with(&source, &Progress::default()).unwrap_err();
    assert!(error.contains("no geometry found"), "{error}");
    let broken = Source::Bytes { name: "broken.samjson".into(), data: Arc::new(b"{ \"format\": \"sam-json\" }".to_vec()) };
    assert!(loaders::load_with(&broken, &Progress::default()).unwrap_err().contains("broken.samjson"));
}

#[test]
fn samjson_is_registered_and_xyz_stays_last() {
    assert!(loaders::supported_extensions().contains(&"samjson"));
    let registry = loaders::registry();
    assert_eq!(registry.last().unwrap().name(), "XYZ");
    assert_eq!(registry.iter().filter(|l| l.extensions().contains(&"samjson")).count(), 1);
}

/// A model of `points` (world metres) as nodes, its anchor at `anchor`, with a mesh layer of one
/// triangle whose first corner lies on the first node.
fn anchored(name: &str, anchor: DVec3, points: &[[f64; 3]]) -> Model {
    let mut doc = AnalysisModel::new(1, name, SourceInfo::default());
    for (k, p) in points.iter().enumerate() {
        doc.nodes.push(Node {
            id: sam_id(IdKind::Node, k as u64 + 1),
            number: (k + 1).to_string(),
            p: DVec3::from_array(*p),
            frame: None,
            support: None,
            origin: bim_structural::NodeOrigin::Analytical,
        });
    }
    doc.members.push(bim_structural::Member {
        id: sam_id(IdKind::Member, 1),
        number: "1".into(),
        kind: bim_structural::MemberKind::Beam,
        nodes: vec![sam_id(IdKind::Node, 1), sam_id(IdKind::Node, 2)],
        geometry: Curve::Line,
        z_ref: DVec3::Z,
        orientation: Orientation::Default,
        section: None,
        section_rotation: 0.0,
        insertion: Default::default(),
        releases: [None, None],
        material: None,
        links: Vec::new(),
    });
    doc.rebuild_index();
    let q = (DVec3::from_array(points[0]) - anchor).as_vec3();
    let layer = Layer {
        name: "Mesh".into(),
        positions: vec![q.x, q.y, q.z, q.x + 1.0, q.y, q.z, q.x, q.y + 1.0, q.z],
        normals: [0.0, 0.0, 1.0].repeat(3),
        indices: vec![0, 1, 2],
        elements: vec![(7, 0, 3)],
        props: vec![(7, format!(r#"{{"name":"{name} mesh"}}"#))],
        ..Layer::default()
    };
    let mut model = Model::new(PathBuf::from(format!("{name}.samjson")), "test", vec![layer]);
    model.origins = vec![crate::model::Origin { anchor: Some(anchor), shift: DVec3::ZERO }];
    model.set_structural(vec![StructuralPart::new(0, Arc::new(doc))]);
    model
}

#[test]
fn combine_keeps_every_structural_id_and_aligns_a_5_km_offset() {
    // the same two world points, seen from anchors 5 km apart
    let points = [[5000.1234, 20.5, 3.25], [5006.5, 20.5, 3.25]];
    let a = Arc::new(anchored("A", DVec3::ZERO, &points));
    let b = Arc::new(anchored("B", DVec3::new(5000.0, 0.0, 0.0), &points));
    let combined = Model::combine(&[a.clone(), b.clone()]);

    // both origins, each with its own anchor
    assert_eq!(combined.origins.len(), 2);
    assert_eq!(combined.origins[0].anchor, Some(DVec3::ZERO));
    assert_eq!(combined.origins[1].anchor, Some(DVec3::new(5000.0, 0.0, 0.0)));
    assert_eq!(combined.origins[1].shift, DVec3::new(5000.0, 0.0, 0.0));

    // every structural object of both models, under unique element ids, back to its source id
    let mut keys = std::collections::HashSet::new();
    for (k, source) in [&a, &b].into_iter().enumerate() {
        for (id, _) in items(&source.structural[0].doc) {
            let part = &combined.structural[k];
            let item = combined.structural_item(part.element_id(id)).expect("every id is indexed");
            assert_eq!((item.source, item.part.model), (id, k as u32));
            assert!(keys.insert(item.element), "element ids are unique");
        }
    }
    assert_eq!(keys.len(), 6);
    assert_eq!(combined.structural_index.len(), 6);
    // mesh elements keep their own ids apart from the structural ones
    assert!(combined.layers.iter().flat_map(|l| &l.elements).all(|e| !keys.contains(&e.0)));

    // aligned within 1 mm: the nodes of both models and the meshes on them
    for n in 0..2 {
        let pa = combined.structural_item(combined.structural[0].element_id(sam_id(IdKind::Node, n + 1))).unwrap().bounds(&combined.origins).unwrap();
        let pb = combined.structural_item(combined.structural[1].element_id(sam_id(IdKind::Node, n + 1))).unwrap().bounds(&combined.origins).unwrap();
        for k in 0..3 {
            assert!((pa.min[k] - pb.min[k]).abs() < 1e-3, "node {n} axis {k}: {pa:?} vs {pb:?}");
        }
    }
    let (la, lb) = (&combined.layers[0].positions, &combined.layers[1].positions);
    for k in 0..la.len() {
        assert!((la[k] - lb[k]).abs() < 1e-3, "mesh coordinate {k}: {} vs {}", la[k], lb[k]);
    }
    // the world point back from the display, in f64
    let o = combined.origins[1];
    let back = o.world(o.display(DVec3::from_array(points[0])));
    assert!((back - DVec3::from_array(points[0])).length() < 1e-9);
}

#[test]
fn combine_leaves_far_models_and_models_without_anchor_in_place() {
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let near = Arc::new(anchored("near", DVec3::ZERO, &points));
    let far = Arc::new(anchored("far", DVec3::new(ALIGN_LIMIT + 1.0, 0.0, 0.0), &points));
    let combined = Model::combine(&[near, far.clone()]);
    assert_eq!(combined.origins[1].shift, DVec3::ZERO);
    assert_eq!(combined.layers[1].positions, far.layers[0].positions);
    assert!(combined.project.info.iter().any(|(k, _)| k == "far.samjson / Not aligned"));

    // models without an anchor (most formats) combine exactly as before
    let plain = |name: &str, x: f32| {
        let layer = Layer {
            name: "L".into(),
            positions: vec![x, 0.0, 0.0, x + 1.0, 0.0, 0.0, x, 1.0, 0.0],
            indices: vec![0, 1, 2],
            elements: vec![(1, 0, 3)],
            ..Layer::default()
        };
        Arc::new(Model::new(name.into(), "OBJ", vec![layer]))
    };
    let (p, q) = (plain("p.obj", 0.0), plain("q.obj", 5.0));
    let combined = Model::combine(&[p.clone(), q.clone()]);
    assert_eq!(combined.layers[0].positions, p.layers[0].positions);
    assert_eq!(combined.layers[1].positions, q.layers[0].positions);
    assert!(combined.structural.is_empty() && combined.structural_index.is_empty());
    assert_eq!(combined.origins, vec![crate::model::Origin::default(); 2]);
}

#[test]
fn explode_units_and_storeys_of_structural_objects() {
    let mut model = anchored("frame", DVec3::ZERO, &[[0.0, 0.0, 0.5], [4.0, 0.0, 3.5]]);
    // storeys 00 (mesh element 7 at 0.5 m) and 01 (a second mesh element 6 at 3.5 m; added after
    // the structural parts, so below their compact ids, which start after 7)
    model.project.levels = vec![("00".into(), 0.0), ("01".into(), 3.0)];
    model.props.insert(7, r#"{"level":"00"}"#.into());
    model.props.insert(6, r#"{"level":"01"}"#.into());
    let mut upper = model.layers[0].clone();
    upper.positions.chunks_exact_mut(3).for_each(|p| p[2] = 3.5);
    upper.elements = vec![(6, 0, 3)];
    model.layers.push(upper);
    let index = ExplodeIndex::new(&model);
    let (n1, n2, m1) = (el(&model, sam_id(IdKind::Node, 1)), el(&model, sam_id(IdKind::Node, 2)), el(&model, sam_id(IdKind::Member, 1)));
    let (u1, u2, um) = (index.structural_unit(n1).unwrap(), index.structural_unit(n2).unwrap(), index.structural_unit(m1).unwrap());
    assert!(u1 != u2 && u2 != um && u1 != um);
    assert_eq!(index.unit_bounds(um).unwrap().max, [4.0, 0.0, 3.5]);
    assert_eq!(index.storeys(), 2);
    // the nearest storey at or below: node 1 and the member (base 0.5 m) on 00, node 2 on 01
    let offsets = index.offsets(crate::explode::ExplodeMode::Storeys, [0.0, 0.0, 1.0], &glam::Mat4::IDENTITY);
    let upper_unit = index.unit(1, 0);
    assert_ne!(offsets[upper_unit], [0.0; 4]);
    assert_eq!(offsets[u2 as usize], offsets[upper_unit], "node 2 moves with storey 01");
    assert_eq!(offsets[u1 as usize], [0.0; 4]);
    assert_eq!(offsets[um as usize], [0.0; 4]);
}

#[test]
fn box_selection_finds_nodes_and_members() {
    let model = loaders::load(&fixture("portal_frame.samjson")).unwrap();
    // around the left column (x = 0): its two nodes and the column, wholly inside
    let inside = |b: &Bounds| b.min[0] >= -0.5 && b.max[0] <= 0.5;
    let mut ids = model.structural_index.ids_in_box(&model.structural, &model.origins, inside);
    ids.sort_unstable();
    let mut want = vec![el(&model, N1), el(&model, N1 + 1), el(&model, sam_id(IdKind::Member, 1))];
    want.sort_unstable();
    assert_eq!(ids, want);

    let mut app = app_with(model);
    let found = run(&mut app, "find_structural", json!({ "within": { "min": [-0.5, -1, -1], "max": [0.5, 1, 5] } })).unwrap();
    assert_eq!(found["total"], json!(3));
    let crossing = run(&mut app, "find_structural", json!({ "within": { "min": [-0.5, -1, -1], "max": [0.5, 1, 5], "crossing": true } })).unwrap();
    // the beam from node 2 crosses the box too
    assert_eq!(crossing["total"], json!(4));
}

#[test]
fn mcp_hides_and_isolates_structural_objects() {
    use crate::units::UNIT_HIDDEN;
    let model = loaders::load(&fixture("portal_frame.samjson")).unwrap();
    let (member, n1) = (el(&model, sam_id(IdKind::Member, 1)), el(&model, N1));
    let (section, load) = (el(&model, model.structural[0].doc.sections[0].id), el(&model, model.structural[0].doc.actions[0].id));
    let mut app = app_with(model);
    let hidden = |app: &App, id: u64| {
        let unit = app.explode_index.as_ref().unwrap().structural_unit(id).expect("a unit") as usize;
        app.units[unit][3] as u8 & UNIT_HIDDEN != 0
    };
    let answer = run(&mut app, "hide_elements", json!({ "ids": [n1, member] })).unwrap();
    assert_eq!(answer["hidden_count"], json!(2));
    assert!(hidden(&app, n1) && hidden(&app, member));
    run(&mut app, "show_all_elements", json!({})).unwrap();
    assert!(!hidden(&app, n1));
    // isolating a node hides the other three nodes and the three members
    let answer = run(&mut app, "isolate_elements", json!({ "ids": [n1] })).unwrap();
    assert_eq!(answer["hidden_count"], json!(6));
    assert!(!hidden(&app, n1) && hidden(&app, member));
    // a section or a load has no unit of its own: nothing to hide, and the error says why
    for id in [section, load] {
        let refused = run(&mut app, "hide_elements", json!({ "ids": [id] })).unwrap_err();
        assert!(refused.contains("only nodes, members and surfaces"), "{refused}");
    }
}

#[test]
fn mcp_tools_answer_on_structural_objects() {
    let model = loaders::load(&fixture("portal_frame.samjson")).unwrap();
    let (member, n1, n4) = (el(&model, sam_id(IdKind::Member, 1)), el(&model, N1), el(&model, N1 + 3));
    let mut app = app_with(model);

    // get_element and find_elements return structural objects
    let e = run(&mut app, "get_element", json!({ "id": n1 })).unwrap();
    assert_eq!(e["structural"]["source_id"], json!(N1.to_string()));
    assert_eq!(e["properties"]["Class"], json!("Structural node"));
    assert_eq!(e["properties"]["Support"], json!("fixed"));
    assert_eq!(e["bounds"]["min"], json!([0.0, 0.0, 0.0]));
    let found = run(&mut app, "find_elements", json!({ "where": { "category": "structural member" } })).unwrap();
    assert_eq!(found["total"], json!(3));
    assert_eq!(found["elements"][0]["layer"], json!("Structural: Portal frame"));
    assert_eq!(found["elements"][0]["properties"]["Section"], json!("HEB 200"));
    let by_text = run(&mut app, "find_elements", json!({ "text": "ipe 300" })).unwrap();
    assert!(by_text["total"].as_u64().unwrap() >= 1);
    assert!(run(&mut app, "zoom_to", json!({ "target": member })).is_ok());
    // structural objects are selected through MCP like any element (phase S3), a node and a
    // member together; the first one's properties show
    let sel = run(&mut app, "select", json!({ "ids": [n1, member], "zoom": true })).unwrap();
    assert_eq!(sel["element"], json!(n1));
    assert_eq!(sel["structural"], json!([n1, member]));
    assert_eq!(sel["properties"]["Class"], json!("Structural node"));
    assert!(sel["bounds"].is_object());
    assert_eq!(app.structural.selected, vec![n1, member]);
    assert!(!app.structural.selected_marks.is_empty());
    assert!(run(&mut app, "select", json!({ "clear": true })).unwrap().is_null());
    assert!(app.structural.selected.is_empty());
    // a section has no place in the model: nothing to select, and the error says why
    let section = { let m = app.model.as_ref().unwrap(); el(m, m.structural[0].doc.sections[0].id) };
    let refused = run(&mut app, "select", json!({ "id": section })).unwrap_err();
    assert!(refused.contains("structural section"), "{refused}");
    let refused = run(&mut app, "measure_between_elements", json!({ "a": member, "b": n1 })).unwrap_err();
    assert!(refused.contains("structural object"), "{refused}");

    let list = run(&mut app, "list_structural", json!({})).unwrap();
    assert_eq!(list["parts"][0]["counts"]["nodes"], json!(4));
    assert_eq!(list["parts"][0]["counts"]["members"], json!(3));
    assert_eq!(list["parts"][0]["counts"]["supports"], json!(2));

    let issues = run(&mut app, "get_structural_issues", json!({ "model": "portal" })).unwrap();
    let named = issues["issues"].as_array().unwrap().iter().find(|i| i["code"] == "named-support-expanded").unwrap().clone();
    let named_node = named["source_object"].as_str().unwrap().parse::<u64>().unwrap();
    assert_eq!(named["object"], json!(el(&app.model.as_ref().unwrap(), named_node)));
    assert!(named["object_label"].as_str().unwrap().starts_with("node "));
    assert!(run(&mut app, "get_structural_issues", json!({ "model": 3 })).is_err());
    // paged and filtered
    let one = run(&mut app, "get_structural_issues", json!({ "limit": 1 })).unwrap();
    assert_eq!(one["returned"], json!(1));
    assert_eq!(one["truncated"], json!(issues["total"].as_u64().unwrap() > 1));
    let by_code = run(&mut app, "get_structural_issues", json!({ "code": "named-support" })).unwrap();
    assert!(by_code["total"].as_u64().unwrap() >= 1);
    assert!(by_code["issues"].as_array().unwrap().iter().all(|i| i["code"] == "named-support-expanded"));
    let about = run(&mut app, "get_structural_issues", json!({ "object": named["object"].clone() })).unwrap();
    assert!(about["total"].as_u64().unwrap() >= 1);
    assert!(about["issues"].as_array().unwrap().iter().all(|i| i["object"] == named["object"]));

    let m = run(&mut app, "get_structural_element", json!({ "id": member.to_string() })).unwrap();
    assert_eq!(m["kind"], json!("member"));
    assert_eq!(m["length"], json!(4.0));
    assert_eq!(m["insertion"]["cardinal_point"], json!(8));
    assert_eq!(m["nodes"][1]["world"], json!([0.0, 0.0, 4.0]));
    assert_eq!(m["frame"]["x"], json!([0.0, 0.0, 1.0]));
    assert!(run(&mut app, "get_structural_element", json!({ "id": 12345 })).is_err());

    let fixed = run(&mut app, "find_structural", json!({ "kind": "node", "support": "fixed" })).unwrap();
    assert_eq!(fixed["ids"], json!([n1]));
    let rollers = run(&mut app, "find_structural", json!({ "kind": "nodes", "support": "roller" })).unwrap();
    assert_eq!(rollers["ids"], json!([n4]));

    // an id handed out goes through an f64 (a JavaScript MCP client) and back to the same object
    let id = rollers["ids"][0].as_u64().unwrap();
    let through_f64: Value = serde_json::from_str(&format!("{}", id as f64)).unwrap();
    let node = run(&mut app, "get_structural_element", json!({ "id": through_f64 })).unwrap();
    assert_eq!((node["kind"].clone(), node["number"].clone()), (json!("node"), json!("4")));
    for (cmd, args) in [
        ("find_structural", json!({})),
        ("find_elements", json!({ "limit": 100 })),
        ("get_structural_issues", json!({})),
        ("get_structural_element", json!({ "id": member })),
        ("get_element", json!({ "id": member })),
    ] {
        assert_ids_survive_f64(&run(&mut app, cmd, args).unwrap());
    }
    let heb = run(&mut app, "find_structural", json!({ "kind": "member", "section": "heb" })).unwrap();
    assert_eq!(heb["total"], json!(2));
    let range = run(&mut app, "find_structural", json!({ "kind": "node", "number_from": 2, "number_to": 3, "limit": 1 })).unwrap();
    assert_eq!((range["total"].clone(), range["returned"].clone(), range["truncated"].clone()), (json!(2), json!(1), json!(true)));
    assert!(run(&mut app, "find_structural", json!({ "kind": "walls" })).is_err());

    let state = run(&mut app, "get_state", json!({})).unwrap();
    assert_eq!(state["model"]["structural"]["nodes"], json!(4));
}

#[test]
fn non_structural_models_are_unchanged() {
    let cube = loaders::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/test-models/test-cube.obj"))).unwrap();
    assert!(cube.structural.is_empty() && cube.structural_index.is_empty());
    assert_eq!(cube.origins, vec![crate::model::Origin::default()]);
    let index = ExplodeIndex::new(&cube);
    assert!(index.structural_unit(1).is_none());
    let mut app = app_with(cube);
    let state = run(&mut app, "get_state", json!({})).unwrap();
    assert!(state["model"].get("structural").is_none());
    assert_eq!(run(&mut app, "list_structural", json!({})).unwrap()["parts"], json!([]));
}

/// A sam-json document of `models`, each `(name, nodes)` with nodes `(number, [x, y, z], named
/// support)`; `origin` left out when `None`.
fn samjson(origin: Option<[f64; 3]>, models: &[(&str, &[(u64, [f64; 3], Option<&str>)])]) -> Source {
    let models: Vec<Value> = models
        .iter()
        .enumerate()
        .map(|(k, (name, nodes))| {
            let nodes: Vec<Value> = nodes
                .iter()
                .map(|(n, p, support)| {
                    let mut node = json!({ "id": sam_id(IdKind::Node, *n), "no": n.to_string(), "p": p, "frame": null });
                    if let Some(s) = support {
                        node["support"] = json!({ "named": s });
                    }
                    node
                })
                .collect();
            json!({ "id": k + 1, "name": name, "nodes": nodes })
        })
        .collect();
    let mut doc = json!({ "format": "sam-json", "version": 1, "source": {}, "units": "SI", "models": models });
    if let Some(o) = origin {
        doc["origin"] = json!(o);
    }
    Source::Bytes { name: "test.samjson".into(), data: Arc::new(doc.to_string().into_bytes()) }
}

#[test]
fn two_analysis_models_of_one_file_keep_apart() {
    // both number a node 1; the second's has a named support, so a note
    let source = samjson(None, &[("Analytical", &[(1, [0.0, 0.0, 0.0], None)]), ("Physical axes", &[(1, [0.0, 0.0, 5.0], Some("pinned"))])]);
    let model = loaders::load_with(&source, &Progress::default()).unwrap();
    assert_eq!(model.origins.len(), 2, "one origin per analysis model");
    let (a, b) = (model.structural[0].element_id(N1), model.structural[1].element_id(N1));
    assert_ne!(a, b);
    let (ia, ib) = (model.structural_item(a).unwrap(), model.structural_item(b).unwrap());
    assert_ne!(ia.key(), ib.key(), "selection keys differ");
    assert_eq!(model.element_bounds(b).unwrap().min, [0.0, 0.0, 5.0]);
    // the note names the node by number and model, not by an id
    let note = model.project.info.iter().find(|(k, _)| k == "Note 1").unwrap().1.clone();
    assert!(note.ends_with("(node 1, Physical axes)"), "{note}");

    // kept apart through combine, with the notes' objects still found
    let other = Arc::new(anchored("other", DVec3::ZERO, &[[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]));
    let combined = Model::combine(&[Arc::new(model), other]);
    assert_eq!(combined.origins.len(), 3);
    let keys: std::collections::HashSet<_> = combined.structural_index.all(&combined.structural).iter().map(|i| i.key()).collect();
    assert_eq!(keys.len(), combined.structural_index.len());
    let mut app = app_with(combined);
    let issues = run(&mut app, "get_structural_issues", json!({ "code": "named-support" })).unwrap();
    let id = issues["issues"][0]["object"].clone();
    let node = run(&mut app, "get_structural_element", json!({ "id": id })).unwrap();
    assert_eq!((node["analysis_model"].clone(), node["world"].clone()), (json!("Physical axes"), json!([0.0, 0.0, 5.0])));
}

#[test]
fn survey_coordinates_without_an_origin_get_an_anchor_and_align() {
    let points = [(1, [155000.0, 463000.0, 0.0], None), (2, [155006.0, 463004.0, 3.5], None)];
    let model = loaders::load_with(&samjson(None, &[("Survey", &points)]), &Progress::default()).unwrap();
    // the middle of the nodes, rounded to metres, so the display keeps millimetres
    assert_eq!(model.origins[0].anchor, Some(DVec3::new(155003.0, 463002.0, 2.0)));
    assert_eq!(model.bounds.min, [-3.0, -2.0, -2.0]);
    assert_eq!(model.bounds.max, [3.0, 2.0, 1.5]);
    assert!(model.project.info.iter().any(|(k, v)| k == "World origin (metres)" && v.contains("chosen")));

    // the same building from a file that gives its origin: aligned on Add, within 1 mm
    let given = loaders::load_with(&samjson(Some([155000.0, 463000.0, 0.0]), &[("Given", &points)]), &Progress::default()).unwrap();
    let combined = Model::combine(&[Arc::new(model), Arc::new(given)]);
    assert!(!combined.project.info.iter().any(|(k, _)| k.ends_with("Not aligned")));
    for k in 0..2 {
        let b = |p: usize| combined.structural_item(combined.structural[p].element_id(N1 + k)).unwrap().bounds(&combined.origins).unwrap();
        for axis in 0..3 {
            assert!((b(0).min[axis] - b(1).min[axis]).abs() < 1e-3, "node {k} axis {axis}");
        }
    }

    // a model near zero keeps zero
    let near = loaders::load_with(&samjson(None, &[("Near", &[(1, [1.0, 2.0, 3.0], None)])]), &Progress::default()).unwrap();
    assert_eq!(near.origins[0].anchor, Some(DVec3::ZERO));
}

#[test]
fn model_bounds_hold_the_bulge_of_arcs() {
    // an arc of 270 degrees from (1,0,0) through (0,1,0) to (0,-1,0): it passes x = -1
    let mut doc = AnalysisModel::new(1, "Arc", SourceInfo::default());
    for (k, p) in [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0]].into_iter().enumerate() {
        doc.nodes.push(Node {
            id: sam_id(IdKind::Node, k as u64 + 1),
            number: (k + 1).to_string(),
            p: DVec3::from_array(p),
            frame: None,
            support: None,
            origin: bim_structural::NodeOrigin::Analytical,
        });
    }
    doc.members.push(bim_structural::Member {
        id: sam_id(IdKind::Member, 1),
        number: "1".into(),
        kind: bim_structural::MemberKind::Beam,
        nodes: vec![sam_id(IdKind::Node, 1), sam_id(IdKind::Node, 2)],
        geometry: Curve::Arc { mid: DVec3::new(0.0, 1.0, 0.0) },
        z_ref: DVec3::Z,
        orientation: Orientation::Default,
        section: None,
        section_rotation: 0.0,
        insertion: Default::default(),
        releases: [None, None],
        material: None,
        links: Vec::new(),
    });
    doc.rebuild_index();
    let mut model = Model::new(PathBuf::from("arc.samjson"), "test", Vec::new());
    model.set_structural(vec![StructuralPart::new(0, Arc::new(doc))]);
    assert!(model.bounds.min[0] < -0.9, "{:?}", model.bounds);
    let member = model.structural[0].element_id(sam_id(IdKind::Member, 1));
    let b = model.element_bounds(member).unwrap();
    for k in 0..3 {
        assert!(model.bounds.min[k] <= b.min[k] && model.bounds.max[k] >= b.max[k], "model {:?} holds member {b:?}", model.bounds);
    }
}

#[test]
fn meshes_with_anchors_combine_as_before_and_an_analysis_model_aligns_to_them() {
    // two anchored mesh models (IFC) 5 m apart: not moved, no note
    let mesh = |name: &str, anchor: DVec3| {
        let layer = Layer {
            name: "L".into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            indices: vec![0, 1, 2],
            elements: vec![(1, 0, 3)],
            ..Layer::default()
        };
        let mut m = Model::new(PathBuf::from(name), "IFC", vec![layer]);
        m.origins = vec![crate::model::Origin { anchor: Some(anchor), shift: DVec3::ZERO }];
        Arc::new(m)
    };
    let (a, b) = (mesh("a.ifc", DVec3::new(155000.0, 463000.0, 0.0)), mesh("b.ifc", DVec3::new(155005.0, 463000.0, 0.0)));
    let combined = Model::combine(&[a.clone(), b.clone()]);
    assert_eq!(combined.layers[1].positions, b.layers[0].positions);
    assert_eq!(combined.origins[1].shift, DVec3::ZERO);
    assert!(!combined.project.info.iter().any(|(k, _)| k.ends_with("Not aligned")));

    // an analysis model of the same site is moved onto the IFC
    let sam = Arc::new(anchored("frame", DVec3::new(155010.0, 463020.0, 0.0), &[[155010.0, 463020.0, 0.0], [155011.0, 463020.0, 0.0]]));
    let combined = Model::combine(&[a, sam]);
    assert_eq!(combined.origins[1].shift, DVec3::new(10.0, 20.0, 0.0));
    let node = combined.structural_item(combined.structural[0].element_id(N1)).unwrap().bounds(&combined.origins).unwrap();
    assert_eq!(node.min, [10.0, 20.0, 0.0]);
}
