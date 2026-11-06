//! sam-json v1: round trip, conformance of the fixture, one rejecting mutation per rule, and
//! the `sam-check` binary.

#![recursion_limit = "512"]

use std::path::PathBuf;
use std::process::Command;

use bim_structural::base64;
use bim_structural::exchange::{self, Rule};
use bim_structural::results::{self, BlockTable};
use bim_structural::support;
use bim_structural::{sam_id, Dof, IdKind, PropsSource, SupportPreset};
use serde_json::{json, Value};

const PORTAL: &str = include_str!("fixtures/portal_frame.samjson");

fn node(n: u64) -> u64 {
    sam_id(IdKind::Node, n)
}
fn member(n: u64) -> u64 {
    sam_id(IdKind::Member, n)
}

fn portal() -> Value {
    serde_json::from_str(PORTAL).unwrap()
}

// ---------------------------------------------------------------------------------------------
// Round trip

#[test]
fn portal_frame_reads() {
    let doc = exchange::read(PORTAL).unwrap();
    assert_eq!(doc.models.len(), 1);
    let m = &doc.models[0];
    assert_eq!((m.nodes.len(), m.members.len(), m.sections.len()), (4, 3, 2));
    // The fixed support and the named roller.
    assert_eq!(m.node(node(1)).unwrap().support.as_ref().unwrap().preset, SupportPreset::Fixed);
    let roller = m.node(node(4)).unwrap().support.as_ref().unwrap();
    assert_eq!(roller.named.as_deref(), Some("roller_x"));
    assert_eq!(support::code(&roller.dofs), "011000");
    assert_eq!(support::preset_name(roller.preset), Some("roller_x"));
    assert!(m.diagnostics.iter().any(|d| d.code == "named-support-expanded" && d.object == Some(node(4))));
    // One hinge at the end of the beam.
    let beam = m.member(member(2)).unwrap();
    assert_eq!(beam.releases[0], None);
    assert_eq!(support::code(&beam.releases[1].as_ref().unwrap().dofs), "111100");
    // Sections at CP 8 with plain ids shifted.
    assert_eq!(beam.insertion.cardinal_point, 8);
    let ipe = m.section(beam.section.unwrap().0).unwrap();
    assert_eq!(ipe.name, "IPE 300");
    assert_eq!(ipe.id, sam_id(IdKind::Section, 2));
    assert_eq!(ipe.props_source, PropsSource::Computed);
    assert!((ipe.props.area - 53.81e-4).abs() / 53.81e-4 < 0.005);
    // One UDL case and one result set with inline blocks.
    assert_eq!(m.load_cases.len(), 1);
    assert_eq!(m.actions.len(), 1);
    let rs = &m.results[0];
    let payload = rs.inline.as_ref().unwrap();
    let mf = payload.member_forces.as_ref().unwrap();
    assert_eq!(mf.member, vec![member(1), member(2), member(3)]);
    assert_eq!(mf.offsets, vec![0, 2, 9, 11]);
    // Beam: My at the hinge is zero, hogging (+My, tension on +z) at the rigid end.
    let my = &mf.comps[4];
    assert_eq!(my[8], 0.0);
    assert!(my[2] > 0.0);
    assert_eq!(payload.reactions.as_ref().unwrap().node, vec![node(1), node(4)]);
}

#[test]
fn round_trip_portal_frame() {
    let m = exchange::read(PORTAL).unwrap();
    let text = exchange::write(&m);
    let again = exchange::read(&text).unwrap();
    assert_eq!(again, m);
    assert_eq!(exchange::write(&again), text);
}

/// A document that uses every variant of the format.
fn everything() -> Value {
    let dofs_mixed = json!(["fixed", {"spring": 1.0e6}, {"tension_only": 2.0e6}, {"compression_only": 3.0e6},
        {"failure": {"k": 1.0e5, "limit": 2.0e4}}, {"slippage": {"k": 1.0e5, "gap": 0.002}}]);
    let block = |kind: results::TableKind, table: BlockTable| base64::encode(&results::encode(&results::ResultBlock { kind, set: 7, table }));
    let member_table = results::MemberTable {
        member: vec![member(1), member(2)],
        offsets: vec![0, 4, 6],
        x: vec![0.0, 1.5, 1.5, 3.0, 0.0, 2.0],
        comps: std::array::from_fn(|c| (0..6).map(|i| (c as f64 + 1.0) * i as f64).collect()),
    };
    let node_table = results::NodeTable { node: vec![node(1), node(2)], comps: std::array::from_fn(|c| vec![c as f64, -(c as f64)]) };
    let env = results::EnvelopeTable {
        member: vec![member(1)],
        offsets: vec![0, 2],
        x: vec![0.0, 3.0],
        max: std::array::from_fn(|c| vec![c as f64, 1.0]),
        min: std::array::from_fn(|c| vec![-(c as f64), -1.0]),
        governing_max: std::array::from_fn(|_| vec![7, 7]),
        governing_min: std::array::from_fn(|_| vec![7, 7]),
    };
    let s = 0.5f64.sqrt();
    json!({
        "format": "sam-json", "version": 1,
        "source": {"application": "test", "global_transform": [1,0,0,0, 0,-1,0,0, 0,0,-1,0, 10,20,30,1],
                   "conventions": {"gravity_was": "+z", "mz_sign_was": "tension_plus_y", "reactions_were": "on_support",
                                   "result_signs": "assumed_canonical"},
                   "eccentricity_policy": "cardinal_cancels_position"},
        "units": "SI", "origin": [100.0, 200.0, 0.0],
        "models": [{
            "id": 3, "name": "All", "dimension": {"2d": {"normal": [0, 1, 0]}}, "kind": "physical_axes",
            "shared_frame": [0,1,0,0, -1,0,0,0, 0,0,1,0, 5,0,0,1],
            "materials": [{"id": 1, "name": "C30/37"}],
            "sections": [
                {"id": 1, "name": "IPN 200", "shape": {"I": {"h": 0.2, "b": 0.09, "tw": 0.0075, "tf": 0.0113, "r": 0.0075, "slope": 0.1396}}},
                {"id": 2, "name": "HEA tapered", "shape": {"AsymI": {"h": 0.3, "bt": 0.12, "bb": 0.2, "tw": 0.008, "tft": 0.01, "tfb": 0.015, "r": 0.01}}},
                {"id": 3, "name": "UPN 200", "shape": {"U": {"h": 0.2, "b": 0.075, "tw": 0.0085, "tf": 0.0115, "r1": 0.0115, "r2": 0.006, "slope": 0.0798}}},
                {"id": 4, "name": "L", "shape": {"L": {"h": 0.1, "b": 0.08, "t": 0.008}}},
                {"id": 5, "name": "T", "shape": {"T": {"h": 0.1, "b": 0.1, "tw": 0.011, "tf": 0.011, "r": 0.011}}},
                {"id": 6, "name": "C", "shape": {"C": {"h": 0.2, "b": 0.07, "t": 0.002, "lip": 0.02, "r": 0.003}}},
                {"id": 7, "name": "Z", "shape": {"Z": {"h": 0.2, "b": 0.07, "tw": 0.002, "tf": 0.002, "r": 0.003}}},
                {"id": 8, "name": "R", "material": 1, "shape": {"Rect": {"b": 0.3, "h": 0.5}}},
                {"id": 9, "name": "O", "shape": {"Circle": {"d": 0.4}}},
                {"id": 10, "name": "CHS", "shape": {"Chs": {"d": 0.1683, "t": 0.0071}}},
                {"id": 11, "name": "RHS", "shape": {"Rhs": {"b": 0.1, "h": 0.2, "t": 0.006, "r_out": 0.012, "r_in": 0.006}}},
                {"id": 12, "name": "poly", "shape": {"polygon": {"outer": [[-0.1, -0.1], [0.1, -0.1], [-0.1, 0.1]], "holes": [[[-0.08, -0.08], [-0.05, -0.08], [-0.08, -0.05]]]}},
                 "props": {"shear_centre": [-0.09, -0.09], "It": 1e-6}},
                {"id": 13, "name": "comp", "shape": {"composite": [
                    {"shape": {"Rect": {"b": 0.2, "h": 0.01}}, "transform": [1, 0, 0, 1, 0, -0.1]},
                    {"shape": {"Circle": {"d": 0.05}}, "transform": [-1, 0, 0, 1, 0, 0.1]}]}},
                {"id": 14, "name": "Mystery 42", "shape": "unknown"}
            ],
            "nodes": [
                {"id": node(1), "no": "1", "p": [0, 0, 0], "frame": [[s, s, 0], [-s, s, 0], [0, 0, 1]],
                 "support": {"dofs": dofs_mixed, "frame": "node"}},
                {"id": node(2), "no": "2", "p": [3, 0, 0], "frame": null,
                 "support": {"dofs": ["fixed", "free", "fixed", {"diagram": [[0, 0], [0.01, 5e4]]}, "free", "free"],
                             "named": "my special", "frame": {"member": member(1)}},
                 "origin": {"derived_from_physical": {"element": 77, "approximate": true}}},
                {"id": node(3), "no": "3", "p": [3, 0, 3], "frame": null, "support": {"named": "nonsense"}},
                {"id": node(4), "no": "4", "p": [0, 0, 3], "frame": null},
                {"id": node(5), "no": "5", "p": [1.5, 0, 4], "frame": null}
            ],
            "members": [
                {"id": member(1), "no": "1", "kind": "truss", "nodes": [node(1), node(2)], "z": [0, 0, 1],
                 "orientation": {"ref_vector": [0, 0, 1]}, "section": [1, 2], "section_rotation": 0.1,
                 "insertion": {"cardinal_point": 15, "profile_position": [0, 1, -1, 0, 0.01, 0.02],
                               "profile_offsets": [[0.0, 0.01], [0.0, 0.02]], "eccentricity": [0.001, -0.002],
                               "end_offsets": {"start": [0.1, 0, 0], "end": [0, 0, 0.2], "frame": "global"}},
                 "releases": [{"named": "hinge_y"}, {"named": "who knows"}], "material": 1, "links": [77]},
                {"id": member(2), "no": "2", "kind": "cable", "nodes": [node(4), node(5), node(3)],
                 "geometry": {"polyline": [[1.5, 0, 4]]}, "z": [0, 1, 0], "orientation": {"ref_node": node(1)}, "section": [9, null]},
                {"id": member(3), "no": "3", "kind": "tension_only", "nodes": [node(1), node(4)],
                 "geometry": {"arc": {"mid": [-0.5, 0, 1.5]}}, "z": [0, 1, 0], "orientation": "ifc_axis", "section": null},
                {"id": member(4), "no": "4", "kind": "compression_only", "nodes": [node(2), node(3)], "z": [1, 0, 0],
                 "orientation": {"beta": {"angle": 0.5, "rule": "vendor"}},
                 "releases": [{"dofs": ["fixed", "fixed", "fixed", "fixed", {"spring": 1e7}, "free"], "named": "semi"}, null]},
                {"id": member(5), "no": "5", "kind": "rigid", "nodes": [node(4), node(3)], "z": [0, 0, 1], "section": [14, null]},
                {"id": member(6), "no": "6", "kind": "spring", "nodes": [node(1), node(3)], "z": [0, 1, 0]},
                {"id": member(7), "no": "7", "kind": "beam", "nodes": [node(2), node(4)], "z": [0, 1, 0], "section": [13, null]}
            ],
            "surfaces": [
                {"id": sam_id(IdKind::Surface, 1), "no": "S1", "kind": "plate", "boundary": [node(1), node(2), node(3), node(4)],
                 "openings": [[node(5), node(3), node(4)]], "thickness": {"uniform": 0.2}, "offset_z": 0.05, "x_dir": [1, 0, 0],
                 "material": 1, "links": [78]},
                {"id": sam_id(IdKind::Surface, 2), "no": "S2", "kind": "shell", "boundary": [node(1), node(2), node(3)],
                 "thickness": {"linear": [[0, 0, 0, 0.2], [3, 0, 0, 0.25], [3, 0, 3, 0.3]]}},
                {"id": sam_id(IdKind::Surface, 3), "no": "S3", "kind": "membrane", "boundary": [node(1), node(2), node(4)], "thickness": "unknown"},
                {"id": sam_id(IdKind::Surface, 4), "no": "S4", "kind": "rigid", "boundary": [node(1), node(2), node(4)], "thickness": "unknown"},
                {"id": sam_id(IdKind::Surface, 5), "no": "S5", "kind": "load_transfer", "boundary": [node(1), node(2), node(4)], "thickness": "unknown"}
            ],
            "line_supports": [
                {"id": sam_id(IdKind::LineSupport, 1), "no": "L1", "along": {"member": member(1)}, "dofs": ["fixed", "fixed", {"spring": 1e8}, "free", "free", "free"]},
                {"id": sam_id(IdKind::LineSupport, 2), "no": "L2", "along": {"surface_edge": {"surface": sam_id(IdKind::Surface, 1), "from": node(1), "to": node(2)}},
                 "named": "pinned", "frame": {"member": member(1)}},
                {"id": sam_id(IdKind::LineSupport, 3), "no": "L3", "along": {"polyline": [[0, 0, 0], [0, 5, 0]]}, "named": "fixed"}
            ],
            "load_cases": [
                {"id": 1, "no": "LC1", "name": "G", "action_type": "permanent", "action_source": "DEAD_LOAD_G", "self_weight": [0, 0, -1]},
                {"id": 2, "no": "LC2", "name": "Q", "action_type": "variable", "action_source": "LIVE_LOAD_Q", "coefficient": 0.9},
                {"id": 3, "no": "LC3", "name": "A", "action_type": "accidental"},
                {"id": 4, "no": "LC4", "name": "U", "action_type": {"user": "snow drift"}}
            ],
            "combinations": [
                {"id": 1, "no": "CO1", "name": "ULS", "purpose": "uls", "terms": [[{"case": 1}, 1.35], [{"case": 2}, 1.5]]},
                {"id": 2, "no": "CO2", "name": "SLS", "purpose": "sls", "terms": [[{"combination": 1}, 1.0]]},
                {"id": 3, "no": "CO3", "name": "ALS", "purpose": "als", "terms": []},
                {"id": 4, "no": "CO4", "name": "X", "purpose": {"user": "fire"}, "terms": []}
            ],
            "actions": [
                {"id": 9, "load": {"combination": 1}, "target": {"node": node(4)}, "frame": "global",
                 "kind": {"nodal_force": {"f": [1, 2, 3], "m": [4, 5, 6]}}, "destabilising": true},
                {"id": 1, "load": {"case": 2}, "target": {"member": member(1)}, "frame": "local",
                 "kind": {"member_point": {"x": 1.0, "f": [0, 0, -1000]}}},
                {"id": 2, "load": {"case": 1}, "target": {"member_span": {"member": member(1), "span": [0.5, 2.5]}}, "frame": "global_projected",
                 "kind": {"member_distributed": {"dir": [0, 0, -1], "dist": {"trapezoidal": {"a": 0.5, "b": 2.5, "q1": 1000, "q2": 2000}}}}},
                {"id": 3, "load": {"case": 1}, "target": {"member": member(1)}, "frame": "global",
                 "kind": {"member_distributed": {"dir": [0, 0, -1], "moment": true, "dist": {"linear": {"q1": 1, "q2": 2}}}}},
                {"id": 4, "load": {"case": 1}, "target": {"member": member(1)}, "frame": "global",
                 "kind": {"member_distributed": {"dir": [0, 0, -1], "dist": {"polygonal": [[0, 1], [1, 3], [3, 2]]}}}},
                {"id": 5, "load": {"case": 1}, "target": {"member": member(1)}, "frame": "global",
                 "kind": {"member_distributed": {"dir": [0, 0, -1], "dist": {"parabolic": 5.0}}}},
                {"id": 6, "load": {"case": 1}, "target": {"member": member(1)}, "frame": "global",
                 "kind": {"member_distributed": {"dir": [0, 0, -1], "dist": {"sine": 5.0}}}},
                {"id": 7, "load": {"case": 3}, "target": {"surface": sam_id(IdKind::Surface, 1)}, "frame": "global",
                 "kind": {"surface": {"dir": [0, 0, -1], "dist": {"uniform": 2000}}}},
                {"id": 8, "load": {"case": 3}, "target": {"surface": sam_id(IdKind::Surface, 2)}, "frame": "global",
                 "kind": {"surface": {"dir": [0, 0, -1], "dist": {"bilinear": [1, 2, 3]}}}},
                {"id": 10, "load": {"case": 3}, "target": {"free": [[0, 0, 0], [1, 0, 0], [1, 1, 0]]}, "frame": "global",
                 "kind": {"surface": {"dir": [0, 0, -1], "dist": {"sampled": [[0, 0, 1], [1, 0, 2], [1, 1, 3]]}}}},
                {"id": 11, "load": {"case": 4}, "target": {"member": member(4)}, "frame": "local",
                 "kind": {"temperature": {"dt": 20, "dt_y": 5}}},
                {"id": 12, "load": {"case": 4}, "target": {"node": node(1)}, "frame": "global",
                 "kind": {"displacement": {"u": [null, null, -0.01, null, null, null]}}}
            ],
            "results": [
                {"id": 7, "no": "CO1", "name": "ULS", "for": {"combination": 1}, "theory": "second_order", "linear": false,
                 "tables": ["member_forces", "member_deformations", "node_displacements", "reactions"],
                 "inline": {"member_forces": block(results::TableKind::MemberForces, BlockTable::Member(member_table.clone())),
                            "member_deformations": block(results::TableKind::MemberDeformations, BlockTable::Member(member_table)),
                            "node_displacements": block(results::TableKind::NodeDisplacements, BlockTable::Node(node_table.clone())),
                            "reactions": block(results::TableKind::Reactions, BlockTable::Node(node_table))}},
                {"id": 8, "no": "LC2", "name": "Q", "for": {"case": 2}, "theory": "third_order", "linear": true, "tables": ["member_forces"]},
                {"id": 9, "no": "ENV", "name": "Envelope", "for": {"envelope": "all ULS"}, "theory": "full_nonlinear", "linear": false,
                 "tables": ["envelope"], "inline": {"envelope": base64::encode(&results::encode(&results::ResultBlock {
                     kind: results::TableKind::Envelope, set: 9, table: BlockTable::Envelope(env) }))}},
                {"id": 10, "no": "R", "name": "first", "for": {"case": 1}, "theory": "first_order", "linear": true, "tables": [], "inline": null}
            ],
            "links": [
                {"analytical": member(1), "physical": 77, "source": "ifc_rel_assigns_to_product"},
                {"analytical": sam_id(IdKind::Surface, 1), "physical": 78, "source": "revit_association"},
                {"analytical": node(2), "physical": 77, "source": {"matched": {"distance": 0.004}}}
            ],
            "diagnostics": [{"object": member(1), "code": "default-local-z", "message": "from the producer"}, {"object": null, "code": "note", "message": ""}]
        }, {
            "id": 4, "name": "Empty", "nodes": [{"id": node(1), "no": "1", "p": [0, 0, 0]}]
        }]
    })
}

#[test]
fn round_trip_every_variant() {
    let text = serde_json::to_string(&everything()).unwrap();
    let report = exchange::check(&text, &[]);
    assert!(report.is_ok(), "{:#?}", report.errors().collect::<Vec<_>>());
    // Unknown names are warnings only (two supports, two releases).
    assert_eq!(report.warnings().filter(|w| w.rule == Rule::Dofs).count(), 4);
    let m = exchange::read(&text).unwrap();
    let again = exchange::read(&exchange::write(&m)).unwrap();
    assert_eq!(again, m);

    let model = &m.models[0];
    // Names: unknown support name stays custom and free; unknown release stays connected.
    let n3 = model.node(node(3)).unwrap().support.as_ref().unwrap();
    assert_eq!(n3.preset, SupportPreset::Custom);
    assert!(n3.dofs.iter().all(|d| *d == Dof::Free));
    let n2 = model.node(node(2)).unwrap().support.as_ref().unwrap();
    assert_eq!(n2.preset, SupportPreset::Custom);
    let m1 = model.member(member(1)).unwrap();
    assert_eq!(support::code(&m1.releases[0].as_ref().unwrap().dofs), "111101");
    assert_eq!(support::code(&m1.releases[1].as_ref().unwrap().dofs), "111111");
    for code in ["named-support-expanded", "unknown-support-name", "named-release-expanded", "unknown-release-name"] {
        assert!(model.diagnostics.iter().any(|d| d.code == code), "{code}");
    }
    // Actions sorted by load: cases first, then the combination.
    assert!(model.actions.windows(2).all(|w| w[0].load <= w[1].load));
    // Imported shear centre for the polygon; the unknown shape has no properties.
    assert_eq!(model.section(sam_id(IdKind::Section, 12)).unwrap().props_source, PropsSource::Imported);
    assert_eq!(model.section(sam_id(IdKind::Section, 14)).unwrap().props_source, PropsSource::Unavailable);
    assert_eq!(model.section(sam_id(IdKind::Section, 13)).unwrap().props_source, PropsSource::ShearCentreApproximate);
}

// ---------------------------------------------------------------------------------------------
// Conformance: the fixture passes; one mutation per rule fails exactly that rule.

#[test]
fn portal_frame_conforms() {
    let report = exchange::check(PORTAL, &[]);
    assert!(report.issues.is_empty(), "{:#?}", report.issues);
}

fn rejects(rule: Rule, mutate: impl FnOnce(&mut Value)) {
    let mut v = portal();
    mutate(&mut v);
    let report = exchange::check(&serde_json::to_string(&v).unwrap(), &[]);
    assert_eq!(report.failed_rules(), vec![rule], "{:#?}", report.issues);
    assert!(exchange::read(&serde_json::to_string(&v).unwrap()).is_err());
}

fn model(v: &mut Value) -> &mut Value {
    &mut v["models"][0]
}

fn mutate_block(v: &mut Value, table: &str, f: impl FnOnce(&mut results::ResultBlock)) {
    let slot = &mut model(v)["results"][0]["inline"][table];
    let mut block = results::decode(&base64::decode(slot.as_str().unwrap()).unwrap()).unwrap();
    f(&mut block);
    *slot = json!(base64::encode(&results::encode(&block)));
}

macro_rules! rule_test {
    ($name:ident, $rule:expr, $mutate:expr) => {
        #[test]
        fn $name() {
            rejects($rule, $mutate);
        }
    };
}

rule_test!(rule_schema, Rule::Schema, |v| model(v)["colour"] = json!("red"));
rule_test!(rule_format, Rule::Format, |v| v["version"] = json!(2));
rule_test!(rule_id_kind, Rule::IdKind, |v| model(v)["nodes"].as_array_mut().unwrap().push(json!({"id": 99, "no": "99", "p": [9, 9, 9]})));
rule_test!(rule_id_unique, Rule::IdUnique, |v| {
    let copy = model(v)["nodes"][1].clone();
    model(v)["nodes"].as_array_mut().unwrap().push(copy);
});
rule_test!(rule_node_ref, Rule::NodeRef, |v| model(v)["members"][0]["nodes"][1] = json!(node(9)));
rule_test!(rule_member_ref, Rule::MemberRef, |v| model(v)["actions"][0]["target"] = json!({"member": member(9)}));
rule_test!(rule_surface_ref, Rule::SurfaceRef, |v| model(v)["actions"][0]["target"] = json!({"surface": sam_id(IdKind::Surface, 1)}));
rule_test!(rule_section_ref, Rule::SectionRef, |v| model(v)["members"][0]["section"] = json!([7, null]));
rule_test!(rule_material_ref, Rule::MaterialRef, |v| model(v)["sections"][0]["material"] = json!(9));
rule_test!(rule_load_ref, Rule::LoadRef, |v| model(v)["actions"][0]["load"] = json!({"case": 9}));
rule_test!(rule_result_ref, Rule::ResultRef, |v| model(v)["results"][0]["for"] = json!({"combination": 1}));
rule_test!(rule_link_ref, Rule::LinkRef, |v| model(v)["links"] = json!([{"analytical": 12345, "physical": 1, "source": "ifc_rel_assigns_to_product"}]));
rule_test!(rule_member_nodes, Rule::MemberNodes, |v| model(v)["members"].as_array_mut().unwrap().push(json!(
    {"id": member(9), "no": "9", "kind": "beam", "nodes": [node(1)], "z": [1, 0, 0]})));
rule_test!(rule_member_z, Rule::MemberZ, |v| model(v)["members"][0]["z"] = json!([0, 0, 1]));
rule_test!(rule_cardinal_point, Rule::CardinalPoint, |v| model(v)["members"][0]["insertion"]["cardinal_point"] = json!(20));
rule_test!(rule_dofs, Rule::Dofs, |v| model(v)["nodes"][2]["support"] = json!({}));
rule_test!(rule_dof_frame, Rule::DofFrame, |v| model(v)["nodes"][0]["support"]["frame"] = json!("node"));
rule_test!(rule_node_frame, Rule::NodeFrame, |v| model(v)["nodes"][1]["frame"] = json!([[1, 0, 0], [0, 1, 0], [0, 0, -1]]));
rule_test!(rule_rigid_transform, Rule::RigidTransform, |v| v["source"]["global_transform"][0] = json!(2.0));
rule_test!(rule_dimension, Rule::Dimension, |v| model(v)["dimension"] = json!({"2d": {"normal": [0, 2, 0]}}));
rule_test!(rule_section_shape, Rule::SectionShape, |v| model(v)["sections"][0]["shape"]["I"]["tw"] = json!(0.3));
rule_test!(rule_surface_boundary, Rule::SurfaceBoundary, |v| model(v)["surfaces"] = json!([
    {"id": sam_id(IdKind::Surface, 1), "no": "S1", "kind": "plate", "boundary": [node(1), node(2)], "thickness": {"uniform": 0.2}}]));
rule_test!(rule_load_position, Rule::LoadPosition, |v| model(v)["actions"][0]["target"] = json!({"member_span": {"member": member(2), "span": [2.0, 9.0]}}));
rule_test!(rule_result_block, Rule::ResultBlock, |v| model(v)["results"][0]["tables"] = json!(["member_forces"]));
rule_test!(rule_stations, Rule::Stations, |v| mutate_block(v, "member_forces", |b| {
    if let BlockTable::Member(t) = &mut b.table {
        t.x[0] = 4.0;
        t.x[1] = 0.0;
    }
}));

#[test]
fn rule_finite() {
    // In text, numbers beyond f64 are refused as non-finite...
    let text = PORTAL.replacen("\"p\": [0, 0, 4]", "\"p\": [0, 0, 1e999]", 1);
    assert_ne!(text, PORTAL);
    assert_eq!(exchange::check(&text, &[]).failed_rules(), vec![Rule::Finite]);
    // ... and in a document built in memory (the FE DLL path), NaN is found with its path.
    let mut doc = exchange::parse(PORTAL).unwrap();
    doc.models[0].actions[0].kind = exchange::wire::Load::MemberDistributed {
        dir: [0.0, 0.0, -1.0],
        moment: false,
        dist: exchange::wire::Distribution::Uniform(f64::NAN),
    };
    let report = exchange::validate(&doc);
    assert_eq!(report.failed_rules(), vec![Rule::Finite]);
    assert!(report.issues[0].path.contains("uniform"), "{}", report.issues[0].path);
}

#[test]
fn every_rule_has_a_rejecting_test() {
    // Keeps this file in step with Rule::ALL: each rule above has its own test.
    let tested = [
        Rule::Schema, Rule::Format, Rule::Finite, Rule::IdKind, Rule::IdUnique, Rule::NodeRef, Rule::MemberRef,
        Rule::SurfaceRef, Rule::SectionRef, Rule::MaterialRef, Rule::LoadRef, Rule::ResultRef, Rule::LinkRef,
        Rule::MemberNodes, Rule::MemberZ, Rule::CardinalPoint, Rule::Dofs, Rule::DofFrame, Rule::NodeFrame,
        Rule::RigidTransform, Rule::Dimension, Rule::SectionShape, Rule::SurfaceBoundary, Rule::LoadPosition,
        Rule::ResultBlock, Rule::Stations,
    ];
    assert_eq!(tested.to_vec(), Rule::ALL.to_vec());
}

// ---------------------------------------------------------------------------------------------
// The sam-check binary

fn tmp(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}

fn sam_check(args: &[&std::path::Path]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_sam-check")).args(args).output().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn sam_check_binary() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/portal_frame.samjson");
    let (code, out) = sam_check(&[&fixture]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("0 error(s), 0 warning(s)"), "{out}");

    // A sidecar block for the set passes; one for an unknown set fails.
    let doc = exchange::read(PORTAL).unwrap();
    let mut block = doc.models[0].results[0].inline.as_ref().unwrap().blocks(1).remove(0);
    let good = tmp("portal.1.samr");
    std::fs::write(&good, results::encode(&block)).unwrap();
    let (code, out) = sam_check(&[&fixture, &good]);
    assert_eq!(code, 0, "{out}");
    block.set = 99;
    let bad = tmp("portal.99.samr");
    std::fs::write(&bad, results::encode(&block)).unwrap();
    let (code, out) = sam_check(&[&fixture, &bad]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("error[result-block]"), "{out}");

    // A mutated document fails with the rule's code.
    let mut v = portal();
    model(&mut v)["members"][1]["z"] = json!([1, 0, 0]);
    let mutated = tmp("portal_bad_z.samjson");
    std::fs::write(&mutated, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let (code, out) = sam_check(&[&mutated]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("error[member-z] /models/0/members/1/z"), "{out}");

    // Usage error.
    let out = Command::new(env!("CARGO_BIN_EXE_sam-check")).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}
