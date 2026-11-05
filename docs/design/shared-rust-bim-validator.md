# Shared Rust viewer and BIM validator

## Product and runtime
One source repository builds two independently installable products: Open 3D Viewer Studio and Open BIM Validator. Both use the existing iced/wgpu renderer, loaders, properties, measurement and section tools. The validator exposes IDS, validation results, BCF and reports. There is no Python/Node/WebView validation runtime in either native product.

The existing published validator remains available until migrated behavior has acceptance evidence. Existing HTTP/API/cloud functionality is not silently represented as available in the native replacement. A migration capability ledger must identify implemented, unsupported and not-yet-migrated behavior explicitly.

## Dependencies
Keep ifc-lite-processing 25.0.1 for geometry, with matching ifc-lite-core 25.0.1 for semantic IFC access. Validate the complete STEP entity graph, including representationless entities, types and relationships; flattened mesh properties cannot be the validation source. Use openbim-ids 0.2.2 without optional audit features for typed IDS XML. Its parser does not perform model validation; implement the evaluator locally. Pin dependency versions in Cargo.lock. Review optional dependencies and licenses before introducing them.

## Structure
- `crates/bim-validator-core`: native/headless validation library, deterministic serializable results, IFC adapter, IDS evaluator, tests; no GUI dependency.
- Existing viewer library: common renderer and UI. Product descriptor selects branding, settings namespace and available tools.
- Separate viewer and validator entrypoints; validation runs on background tasks and links findings to source model plus GlobalId/express ID.
- Independent installer/update identifiers and artifact names, built from the same source revision. Windows native iced applications do not require WebView2.

## Correctness and migration
Cover six IDS facets, required/optional/prohibited cardinality, schema gating, type inheritance, data types and unit normalization, XSD restrictions, deterministic findings and invalid-input errors. Any unsupported construct must be explicit, never a pass. Regression fixtures and the official IDS corpus determine release readiness. The existing Python implementation may be used only as a development oracle, never shipped or called at runtime.

BCF round trips must preserve IDs, metadata, comments, cameras and assets; results export must be available headlessly. Export capabilities with remaining limitations are labelled precisely.

## Plan
1. Implement native validation core using existing IFC crates; run focused and corpus tests.
2. Add separate app entrypoints/product identity and package recipes with collision-free installations.
3. Integrate validation actions/results into the native app; preserve model identity and cancellation/stale-result guards.
4. Add BCF/report interoperability and command-line access.
5. Run clean builds, existing viewer tests, semantic regression tests and real native UI verification; fix regressions.
6. Prepare reviewable PRs, migration ledger and release evidence. Publish only capabilities demonstrated by tests, never claim complete parity without it.
