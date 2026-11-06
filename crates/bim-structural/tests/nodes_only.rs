//! The nodes-only fixture: a file of analysis nodes without members passes the conformance check
//! and reads, with its origin kept apart from the world coordinates of the nodes.

use bim_structural::exchange;

const NODES_ONLY: &str = include_str!("fixtures/nodes_only.samjson");

#[test]
fn nodes_only_fixture_conforms_and_reads() {
    let report = exchange::check(NODES_ONLY, &[]);
    assert!(report.is_ok(), "{:#?}", report.errors().collect::<Vec<_>>());
    let doc = exchange::read(NODES_ONLY).expect("reads");
    assert_eq!(doc.origin, glam::DVec3::new(155000.0, 463000.0, 0.0));
    let model = &doc.models[0];
    assert_eq!((model.nodes.len(), model.members.len()), (4, 0));
    // world metres, the origin not subtracted
    assert_eq!(model.nodes[3].p, glam::DVec3::new(155000.0, 463004.0, 3.5));
    // the named support is expanded, with a note
    assert!(model.nodes[1].support.as_ref().is_some_and(|s| s.named.as_deref() == Some("pinned")));
    assert!(model.diagnostics.iter().any(|d| d.code == "named-support-expanded"));
    assert_eq!(exchange::read(&exchange::write(&doc)).unwrap(), doc);
}
