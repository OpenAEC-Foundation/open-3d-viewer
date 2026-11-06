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
    // One hinge at the start of the beam; the beam and the right column form a rigid L on the
    // roller, so the frame is stable (no mechanism) and statically determinate.
    let beam = m.member(member(2)).unwrap();
    assert_eq!(support::code(&beam.releases[0].as_ref().unwrap().dofs), "111100");
    assert_eq!(beam.releases[1], None);
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
    // Beam: My is zero at the hinge and at the rigid end (the column on the roller carries no
    // moment), and sagging at mid-span: -qL²/8 (tension on the -z fibre). Vz = qx - qL/2.
    let my = &mf.comps[4];
    assert_eq!(my[2], 0.0);
    assert_eq!(my[8], 0.0);
    assert_eq!(my[5], -5000.0 * 36.0 / 8.0);
    assert_eq!(mf.comps[2][2..9], [-15000.0, -10000.0, -5000.0, 0.0, 5000.0, 10000.0, 15000.0]);
    // Both columns in compression, qL/2 each; the reactions balance the load.
    assert_eq!([mf.comps[0][0], mf.comps[0][9]], [-15000.0, -15000.0]);
    let reactions = payload.reactions.as_ref().unwrap();
    assert_eq!(reactions.node, vec![node(1), node(4)]);
    assert_eq!(reactions.comps[2].iter().sum::<f64>(), 5000.0 * 6.0);
    assert!(reactions.comps[4].iter().all(|&m| m == 0.0));
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
                    {"shape": {"Circle": {"d": 0.05}}, "transform": [-1, 0, 0, 1, 0, 0.08]}]}},
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
    // The approximation is noted once, also after a write and read.
    let noted = |m: &bim_structural::AnalysisModel| {
        m.diagnostics.iter().filter(|d| d.code == "shear-centre-approximated").map(|d| d.object).collect::<Vec<_>>()
    };
    assert_eq!(noted(model), vec![Some(sam_id(IdKind::Section, 13))]);
    assert_eq!(noted(&again.models[0]), noted(model));
}

/// Every f64 survives write and read bit for bit, not only short decimals.
#[test]
fn round_trip_arbitrary_floats() {
    // A fixed xorshift sequence of f64 with full 53-bit mantissas over several magnitudes.
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let unit = (state >> 11) as f64 / (1u64 << 53) as f64;
        let scale = [1e-3, 1.0, 7.0, 1e3, 1e6][(state % 5) as usize];
        (unit * 2.0 - 1.0) * scale
    };
    let mut v = portal();
    let mut values = Vec::new();
    for k in 0..2000u64 {
        let p = [next(), next(), next()];
        values.push(p);
        model(&mut v)["nodes"].as_array_mut().unwrap().push(json!({"id": node(100 + k), "no": format!("{}", 100 + k), "p": p}));
    }
    // Node 3 off the frame plane, as in the reported case; z stays perpendicular to both axes.
    model(&mut v)["nodes"][2]["p"][1] = json!(1.3181363305521863);
    let doc = exchange::read(&serde_json::to_string(&v).unwrap()).unwrap();
    let m = &doc.models[0];
    for (k, p) in values.iter().enumerate() {
        let got = m.node(node(100 + k as u64)).unwrap().p.to_array();
        assert_eq!(got.map(f64::to_bits), p.map(f64::to_bits), "node {}", 100 + k);
    }
    assert_eq!(m.node(node(3)).unwrap().p.y.to_bits(), 1.3181363305521863f64.to_bits());
    assert!(exchange::read(&exchange::write(&doc)).unwrap() == doc);
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
rule_test!(rule_surface_ref, Rule::SurfaceRef, |v| model(v)["line_supports"] = json!([{"id": sam_id(IdKind::LineSupport, 1), "no": "L1",
    "along": {"surface_edge": {"surface": sam_id(IdKind::Surface, 1), "from": node(1), "to": node(2)}}, "named": "pinned"}]));
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
rule_test!(rule_load_target, Rule::LoadTarget, |v| model(v)["actions"][0]["target"] = json!({"node": node(2)}));
rule_test!(rule_result_block, Rule::ResultBlock, |v| model(v)["results"][0]["tables"] = json!(["member_forces"]));
rule_test!(rule_stations, Rule::Stations, |v| mutate_block(v, "member_forces", |b| {
    if let BlockTable::Member(t) = &mut b.table {
        t.x[0] = 4.0;
        t.x[1] = 0.0;
    }
}));

/// Unknown fields are refused inside enum variants too (shapes, loads, DOFs, targets, ...).
#[test]
fn schema_rejects_unknown_fields_in_variants() {
    type Mutation = fn(&mut Value);
    let cases: [(&str, Mutation); 6] = [
        ("shape", |v| {
            let i = model(v)["sections"][1]["shape"]["I"].as_object_mut().unwrap();
            let r = i.remove("r").unwrap();
            i.insert("radius".into(), r);
        }),
        ("load", |v| model(v)["actions"][0]["kind"]["member_distributed"]["momnet"] = json!(true)),
        ("distribution", |v| model(v)["actions"][0]["kind"]["member_distributed"]["dist"] = json!({"linear": {"q1": 1, "q2": 2, "q3": 3}})),
        ("orientation", |v| model(v)["members"][0]["orientation"]["beta"]["extra"] = json!(1)),
        ("target", |v| model(v)["actions"][0]["target"] = json!({"member_span": {"member": member(2), "span": [0, 1], "x": 0}})),
        ("dof", |v| model(v)["nodes"][0]["support"]["dofs"][0] = json!({"failure": {"k": 1, "limit": 2, "gap": 3}})),
    ];
    for (what, mutate) in cases {
        let mut v = portal();
        mutate(&mut v);
        let text = serde_json::to_string(&v).unwrap();
        assert_eq!(exchange::check(&text, &[]).failed_rules(), vec![Rule::Schema], "{what}");
        assert!(exchange::read(&text).is_err(), "{what}");
    }
}

#[test]
fn gravity_flag_matches_transform() {
    // "+z" without the rotation, and the rotation without "+z", both fail.
    let rot_x_180 = json!([1, 0, 0, 0, 0, -1, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1]);
    rejects(Rule::RigidTransform, |v| v["source"]["conventions"]["gravity_was"] = json!("+z"));
    let r = rot_x_180.clone();
    rejects(Rule::RigidTransform, move |v| v["source"]["global_transform"] = r);
    // Both together pass (a plan rotation about Z keeps "-z" valid as well).
    let mut v = portal();
    v["source"]["conventions"]["gravity_was"] = json!("+z");
    v["source"]["global_transform"] = rot_x_180;
    assert!(exchange::check(&v.to_string(), &[]).is_ok());
    let mut v = portal();
    v["source"]["global_transform"] = json!([0, 1, 0, 0, -1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
    assert!(exchange::check(&v.to_string(), &[]).is_ok());
}

#[test]
fn free_outlines_are_centred() {
    // A polygon with its origin at a corner is refused; the same polygon centred passes.
    rejects(Rule::SectionShape, |v| model(v)["sections"][1]["shape"] = json!({"polygon": {"outer": [[0, 0], [0.15, 0], [0.15, 0.3], [0, 0.3]]}}));
    let mut v = portal();
    model(&mut v)["sections"][1]["shape"] = json!({"polygon": {"outer": [[-0.075, -0.15], [0.075, -0.15], [0.075, 0.15], [-0.075, 0.15]]}});
    assert!(exchange::check(&v.to_string(), &[]).is_ok());
    // An off-centre composite too.
    rejects(Rule::SectionShape, |v| model(v)["sections"][1]["shape"] = json!({"composite": [
        {"shape": {"Rect": {"b": 0.2, "h": 0.01}}, "transform": [1, 0, 0, 1, 0, -0.1]},
        {"shape": {"Circle": {"d": 0.05}}, "transform": [1, 0, 0, 1, 0, 0.1]}]}));
}

#[test]
fn extreme_dimensions_are_refused() {
    // Finite, positive and fitting, but the property integrals overflow.
    rejects(Rule::SectionShape, |v| model(v)["sections"][1]["shape"] = json!({"I": {"h": 1e200, "b": 1e199, "tw": 1e197, "tf": 1e198}}));
}

#[test]
fn shear_centre_needed_for_cp_15_to_19() {
    let triangle = json!({"polygon": {"outer": [[-0.1, -0.15], [0.1, -0.15], [-0.1, 0.15]]}});
    for cp in [15, 19] {
        let t = triangle.clone();
        rejects(Rule::CardinalPoint, move |v| {
            model(v)["sections"][1]["shape"] = t;
            model(v)["sections"][1]["props"] = json!({"A": 0.03});
            model(v)["members"][1]["insertion"]["cardinal_point"] = json!(cp);
        });
    }
    // With an imported shear centre, or at another cardinal point, it passes.
    let mut v = portal();
    model(&mut v)["sections"][1]["shape"] = triangle.clone();
    model(&mut v)["members"][1]["insertion"]["cardinal_point"] = json!(15);
    model(&mut v)["sections"][1]["props"] = json!({"shear_centre": [-0.09, -0.14]});
    assert!(exchange::check(&v.to_string(), &[]).is_ok());
    let mut v = portal();
    model(&mut v)["sections"][1]["shape"] = triangle;
    model(&mut v)["sections"][1]["props"] = json!({});
    let doc = exchange::read(&v.to_string()).unwrap();
    // At CP 8 it reads, with the approximation noted.
    let id = sam_id(IdKind::Section, 2);
    assert!(doc.models[0].diagnostics.iter().any(|d| d.code == "shear-centre-approximated" && d.object == Some(id)));
}

#[test]
fn load_kinds_fit_their_targets() {
    rejects(Rule::LoadTarget, |v| model(v)["actions"][0] = json!({"id": 1, "load": {"case": 1}, "target": {"node": node(2)},
        "frame": "global", "kind": {"member_point": {"x": 99.0, "f": [0, 0, -1]}}}));
    rejects(Rule::LoadTarget, |v| model(v)["actions"][0]["kind"] = json!({"surface": {"dir": [0, 0, -1], "dist": {"uniform": 1}}}));
    rejects(Rule::LoadTarget, |v| model(v)["actions"][0]["kind"] = json!({"nodal_force": {"f": [0, 0, -1]}}));
}

#[test]
fn member_nodes_never_repeat() {
    rejects(Rule::MemberNodes, |v| model(v)["members"][1]["nodes"] = json!([node(2), node(3), node(2), node(4), node(3)]));
    rejects(Rule::MemberNodes, |v| model(v)["members"][1]["nodes"] = json!([node(2), node(1), node(1), node(3)]));
}

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
        Rule::LoadTarget, Rule::ResultBlock, Rule::Stations,
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

    // Usage error; help asked for is a success.
    let out = Command::new(env!("CARGO_BIN_EXE_sam-check")).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    for flag in ["-h", "--help"] {
        let out = Command::new(env!("CARGO_BIN_EXE_sam-check")).arg(flag).output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{flag}");
        assert!(String::from_utf8_lossy(&out.stdout).starts_with("usage: sam-check"), "{flag}");
    }
}

/// Result-set ids are unique per model only: a sidecar block is accepted when it fits one of the
/// models that have its set.
#[test]
fn sidecar_block_with_a_set_in_two_models() {
    let mut v = portal();
    let mut second = model(&mut v).clone();
    second["id"] = json!(2);
    // Model 2 has only the beam, as member 7, and its own result set 1.
    second["members"] = json!([{"id": member(7), "no": "7", "kind": "beam", "nodes": [node(2), node(3)], "z": [0, 0, 1]}]);
    second["actions"] = json!([]);
    second["results"][0]["inline"] = json!(null);
    v["models"].as_array_mut().unwrap().push(second);
    let doc = exchange::parse(&v.to_string()).unwrap();
    assert!(exchange::validate(&doc).is_ok());

    let read = exchange::read(PORTAL).unwrap();
    let mut block = read.models[0].results[0].inline.as_ref().unwrap().blocks(1).remove(0);
    let bytes = results::encode(&block);
    // Fits model 1 (listed first) ...
    assert!(exchange::validate_block(&doc, "portal.1.samr", &bytes).is_ok());
    // ... and a block for model 2's member fits model 2 although model 1 also has set 1.
    if let BlockTable::Member(t) = &mut block.table {
        *t = results::MemberTable { member: vec![member(7)], offsets: vec![0, 2], x: vec![0.0, 6.0], comps: std::array::from_fn(|_| vec![0.0, 0.0]) };
    }
    let report = exchange::validate_block(&doc, "portal.1.samr", &results::encode(&block));
    assert!(report.is_ok(), "{:#?}", report.issues);
    // A block that fits neither says so.
    if let BlockTable::Member(t) = &mut block.table {
        t.member = vec![member(8)];
    }
    let report = exchange::validate_block(&doc, "portal.1.samr", &results::encode(&block));
    assert!(report.errors().any(|i| i.message.contains("in 2 models")), "{:#?}", report.issues);
}
