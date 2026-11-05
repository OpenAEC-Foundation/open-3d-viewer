# Native IFC / IDS validation core

`validate(ifc_bytes, ids_xml)` and `validate_with_cancel(ifc_bytes, ids_xml, &AtomicBool)` return a deterministic, serde-compatible `ValidationReport`. They require no Python, Node, network service, geometry processing or GUI. Entity and relationship data come from every STEP record, including representationless objects and non-rooted resources.

A result is `passed`, `failed`, or `not_checkable`. Unknown semantics dominate the aggregate result, including when another requirement fails. Malformed input is an error. Findings contain express ID, optional GlobalId, entity class, facet, status and reason. Specifications preserve document order; objects preserve numeric express-ID order. Cancellation is cooperative between scan/evaluation iterations.

## Implemented scope

- IDS 1.0 typed XML parsing with pinned `openbim-ids =0.2.2`, without optional audit dependencies.
- Source-schema-specific IFC2X3/IFC4/IFC4X3 entity inheritance and positional attributes through `ifc-lite-core =25.0.1`. Specifications are gated on their declared schema labels. IFC2X3 occurrence names derived from assigned types are mapped where the IFC4 inheritance graph confirms the occurrence class.
- Entity/predefined-type facets, including type assignments and user-defined ObjectType/ElementType/ProcessType.
- Attribute existence and scalar/list comparison; referenced-object existence checks.
- Property and quantity sets, type property inheritance with occurrence overrides, material/profile property sets, single, list, enumeration, bounded and table values; simple length/area/volume/weight/time/count quantities.
- SI prefixes, conversion-based and derived unit scales, explicit property units, dimensional type checking, project-unit lookup. Values are normalized to SI before comparison. Missing/ambiguous units fail closed.
- Material assignment graphs, classification reference ancestry and resource external references, and transitive aggregate/nest/group/spatial-containment partOf relationships. Group class matching is exact.
- Required, optional and prohibited requirements and specification occurrence bounds. Optional present-but-wrong data fails. All matching property sets must satisfy a property requirement.
- XSD 1.0 patterns using `oxixml-regex =0.1.2` (Apache-2.0), including character subtraction and XML name escapes. Enumeration alternatives, numeric inclusive/exclusive bounds and string lengths. Floating equality uses the IDS fixed-plus-relative tolerance; integer scalar equality is exact.

## Remaining limits

This is not complete IDS conformance or a complete IFC schema auditor. Explicit `not_checkable` cases include combined void/fill partOf, derived attribute evaluation, reference/binary value comparison, predefined/complex/reference property sets, explicit table-property units, temperature offsets, monetary conversion, unresolved units, unsupported XSD base types and digit facets. Numeric restrictions use finite f64; integer restrictions outside the exactly representable range are refused. XML Schema datatype lexical validation and normalization are not a complete general-purpose XSD implementation.

Structural admission rejects unknown entity classes, incorrect attribute arity, duplicate IDs, malformed records and excessive nesting. Duplicate matching properties at one ownership level and cyclic partOf/classification graphs are ambiguous and do not pass. Graph walks are bounded. IFC input is limited to 2 GiB / 5 million entities, STEP nesting to 64, IDS to 8 MiB, regex patterns to 4096 bytes and match values to 1 MiB. XML DTD/entity declarations are rejected. The semantic graph is currently eagerly decoded; very large files require substantial memory and have not been benchmarked for release readiness. Cancellation does not interrupt an individual dependency parser/regex operation.

## Verification evidence

On 2026-11-05:

- `cargo test -p bim-validator-core --lib`: **17 passed**.
- `cargo clippy -p bim-validator-core --all-targets -- -D warnings`: **passed**.
- Official [buildingSMART IDS test cases](https://github.com/buildingSMART/IDS/tree/870f9c4e6e8f414e737b4d84ca1aa9b46fc6c8f3/Documentation/ImplementersDocumentation/TestCases), commit `870f9c4e6e8f414e737b4d84ca1aa9b46fc6c8f3`: **307 paired pass/fail cases**, **292 exact expected statuses** (180 passed, 112 failed), **14 not_checkable**, **1 malformed-input error**, **zero false passes and zero opposite pass/fail decisions**.

The 14 unresolved cases comprise nine IDS/IFC schema-label mismatches, four predefined/complex/reference-property cases, and one XSD pattern containing an invalid escaped slash. The malformed-input case has an IFCSPACE with 12 attributes where its IFC4 schema declares 11. They remain explicit unresolved/rejected outcomes; they are not counted as successful corpus cases. The harness excludes `invalid-*` cases and cases without a paired IFC; this evidence is not whole-corpus certification.

Reproduce from the repository root in PowerShell (the corpus stays outside the repository):

```powershell
cargo test -p bim-validator-core --lib
cargo clippy -p bim-validator-core --all-targets -- -D warnings
$corpusDir = Join-Path $env:TEMP 'buildingSMART-IDS-corpus'
git clone https://github.com/buildingSMART/IDS.git $corpusDir
git -C $corpusDir checkout 870f9c4e6e8f414e737b4d84ca1aa9b46fc6c8f3
$casesDir = Join-Path $corpusDir 'Documentation/ImplementersDocumentation/TestCases'
$resultFile = Join-Path $env:TEMP 'bim-validator-corpus.json'
cargo run -p bim-validator-core --example corpus -- $casesDir > $resultFile
$results = Get-Content $resultFile -Raw | ConvertFrom-Json
$results | Group-Object actual,matched | Select-Object Count,Name
```

The example emits every expected/actual status plus complete reports or parse errors for review. Source fixtures are not vendored.
