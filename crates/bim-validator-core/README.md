# Native IFC / IDS validation core

`validate(ifc_bytes, ids_xml)` and `validate_with_cancel(ifc_bytes, ids_xml, &AtomicBool)` return a deterministic, serde-compatible `ValidationReport`. `validate_model(ifc_bytes, &ValidationOptions, &AtomicBool)` evaluates any combination of an IDS, the built-in model checks and the BIM basis ILS profile over one parsed model. They require no Python, Node, network service, geometry processing or GUI. Entity and relationship data come from every STEP record, including representationless objects and non-rooted resources.

A result is `passed`, `failed`, or `not_checkable`. Unknown semantics dominate the aggregate result, including when another requirement fails. Each specification has a `source` (`ids`, `model_checks`, `ils`) and a `severity` (`error`, `warning`); the report status aggregates error-severity specifications only, so warnings are reported but never fail a model. Malformed input is an error. Findings contain express ID, optional GlobalId, entity class, name, building storey (through containment or decomposition), facet, status and reason. Specifications are ordered IDS (document order), model checks, ILS rules; objects preserve numeric express-ID order. Every report carries a `model` summary (element count, proxies, elements without storey/material/NL-SfB/type, length unit, storeys by elevation). Cancellation is cooperative between scan/evaluation iterations.

## Built-in model checks (`model_checks`)

No IDS and no geometry are needed. Codes refer to the [buildingSMART validation service rules](https://buildingsmart.github.io/ifc-gherkin-rules/branches/main/features/index.html) where one exists; `OBV-` codes are Open BIM Validator industry-practice checks.

| Code | Severity | Check |
|---|---|---|
| PJS101 | error | Exactly one IfcProject |
| SPS001 | error (IFC2X3), warning (IFC4+) | At least one IfcBuilding/IfcFacility; IFC2X3 at most one IfcSite. bSI scopes SPS001 to IFC2X3; later schemas get the building check as a warning |
| SPS002 | error | Spatial elements aggregated by a permitted parent per bSI `spatial_CompositionTable.csv` (Project→Site→Building→Storey→Space, facilities). Also applied to IFC2X3 |
| SPS007 | error | IfcElement (except features and decomposed parts) contained in a spatial structure; parts aggregated into an element not also contained; IfcGrid/IfcAnnotation contained; other products not contained; at most one container |
| PJS003 | error | GlobalId is 22 characters of `0-9A-Za-z_$`, first character 0–3 |
| IfcRoot.UR1 | error | GlobalId unique across IfcRoot instances (IFC schema uniqueness rule) |
| OJT001 | error | USERDEFINED needs ObjectType (occurrence) / ElementType (type); an occurrence must not override a type PredefinedType other than NOTDEFINED. Deviation: occurrence values NOTDEFINED or equal to the type value are accepted; applied to all schemas |
| PJS001 | error | Project conversion-based length/area/volume/angle units: name and factor per bSI `valid_ConversionBasedUnits.csv` (names case-insensitive, factor within 1e-4 relative), SI unit component |
| OBV-UNIT01 | error | Project declares a resolvable LENGTHUNIT; the unit is reported |
| OBV-UNIT02 | warning | Project declares AREAUNIT, VOLUMEUNIT, PLANEANGLEUNIT |
| PSE002 | warning | Property sets named like `pset…` but not exactly `Pset_` (also `Qto_` for quantity sets). The standard set list is not shipped, so custom use of a correctly cased `Pset_` prefix is not detected |
| GRP001 | error | No cycles in IfcRelAssignsToGroup |
| OBV-CYCLE | error | No cycles in IfcRelAggregates/IfcRelNests |
| OBV-PROXY | warning | IfcBuildingElementProxy elements, with share |
| OBV-MATERIAL | warning | Material association on element, type or aggregated parts |
| OBV-TYPE | warning | Type object via IfcRelDefinesByType |

Checked elements for the element rules and the summary are IfcElement occurrences except feature (opening) and virtual elements and parts aggregated into another element.

## BIM basis ILS profile (`ils`)

A built-in rule set after the digiGO [BIM basis ILS](https://www.digigo.nu/en/?p=13079). `ILS-` codes name the topic, not an official clause number. Patterns use XSD regex syntax (implicitly anchored) and are configurable through `IlsOptions`.

| Code | Severity | Check |
|---|---|---|
| ILS-FILE | warning | File name matches `<project>_<discipline>_<part>.ifc` (default `[^_]+_[^_]+_[^_]+\.[iI][fF][cC]`); not checkable without a file name |
| ILS-STOREY-NAME | error | Storey names match `-?[0-9]{2}[a-zA-Z]? \S.*` (`00 begane grond`, `-01 kelder`, `01a entresol`) |
| ILS-STOREY-ELEV | error | Elevation present and no two storeys of one building within 1 mm |
| ILS-NLSFB | error | Element or type has an IfcClassificationReference in a system named NL-SfB (`NL-SfB`, `NLSfB`, …) with code `21` or `21.12` (optional parentheses) |
| ILS-MATERIAL | error | Material association (element, type or parts) |
| ILS-LOADBEARING | error | `Pset_<Class>Common.LoadBearing` boolean on beams, columns, members, plates, slabs, walls; IFC4+ also footings, piles, ramps, roofs, stairs, chimneys. Occurrence values override type values |
| ILS-ISEXTERNAL | error | `Pset_<Class>Common.IsExternal` boolean on walls, curtain walls, slabs, roofs, windows, doors |
| ILS-FIRERATING | error | Where `Pset_*Common.FireRating` exists it is whole minutes (`60`, `60 min`, `60 minuten`); `EI 60` and similar are flagged |
| ILS-PROXY | warning | IfcBuildingElementProxy elements |

Not covered: georeferencing/origin, coordinates, object naming conventions and other ILS topics that need geometry or project-specific agreements.

## Reports

`report_html(&report, title, ReportLanguage)` renders a self-contained single HTML file (inline CSS, no scripts or external assets; English or Dutch labels): summary counts, model-quality numbers, storeys by elevation, a rule table, and per-rule finding tables (express ID, GlobalId, class, name, storey, reason; at most 5000 rows per rule, JSON/CSV contain all). The application's CSV adds `code, severity, entity_type, name, storey` columns after the original five. BCF export can create one topic per finding, per rule, or per rule and storey, with all affected valid GlobalIds as components.

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

## Application and CLI

The Open BIM Validator panel has "Modelcontroles" and "BIM basis ILS" switches next to the IDS picker (an IDS is optional), an editable storey-name pattern, HTML export next to JSON/CSV, and a BCF grouping toggle. Findings of all rule sets appear in one list with rule code, severity, name and storey.

```powershell
open-bim-validator --validate MODEL.ifc [--ids RULES.ids] [--checks] [--ils] [--storey-pattern XSD-PATTERN] `
  [--report RESULT.json|RESULT.csv] [--html REPORT.html] [--lang en|nl] `
  [--bcf ISSUES.bcfzip] [--bcf-group finding|rule|rule-storey]
```

At least one of `--ids`, `--checks`, `--ils` is required. Exit codes: 0 passed, 1 failed (error-severity rules only), 2 not checkable, 3 input/output error. The ILS file-name rule uses the IFC file name.

## Remaining limits

This is not complete IDS conformance or a complete IFC schema auditor. Explicit `not_checkable` cases include combined void/fill partOf, derived attribute evaluation, reference/binary value comparison, predefined/complex/reference property sets, explicit table-property units, temperature offsets, monetary conversion, unresolved units, unsupported XSD base types and digit facets. Numeric restrictions use finite f64; integer restrictions outside the exactly representable range are refused. XML Schema datatype lexical validation and normalization are not a complete general-purpose XSD implementation.

Structural admission rejects unknown entity classes, incorrect attribute arity, duplicate IDs, malformed records and excessive nesting. Duplicate matching properties at one ownership level and cyclic partOf/classification graphs are ambiguous and do not pass. Graph walks are bounded. IFC input is limited to 2 GiB / 5 million entities, STEP nesting to 64, IDS to 8 MiB, regex patterns to 4096 bytes and match values to 1 MiB. XML DTD/entity declarations are rejected. The semantic graph is currently eagerly decoded; very large files require substantial memory and have not been benchmarked for release readiness. Cancellation does not interrupt an individual dependency parser/regex operation.

## Verification evidence

On 2026-11-05:

- `cargo test -p bim-validator-core --lib`: **33 passed** (17 IDS, 16 model-check/ILS/HTML tests on hand-written STEP snippets in `src/tests_rules.rs`).
- `cargo clippy -p bim-validator-core --all-targets -- -D warnings`: **passed**.
- Official [buildingSMART IDS test cases](https://github.com/buildingSMART/IDS/tree/870f9c4e6e8f414e737b4d84ca1aa9b46fc6c8f3/Documentation/ImplementersDocumentation/TestCases), commit `870f9c4e6e8f414e737b4d84ca1aa9b46fc6c8f3`: **307 paired pass/fail cases**, **292 exact expected statuses** (180 passed, 112 failed), **14 not_checkable**, **1 malformed-input error**, **zero false passes and zero opposite pass/fail decisions**. After adding the built-in rule sets, every case status and every specification status/finding count is identical to the IDS-only implementation (306 specifications compared).
- CLI smoke run (`--checks --ils --html --bcf-group rule-storey`) on a 6.7 MB Revit-derived IFC4 model (9,498 entities, 456 checked elements, 6 storeys): 4.4 s in a debug build; findings were plausible for a model without type objects, property sets or NL-SfB classification. This is not a benchmark and not a review against the buildingSMART validation service's own output.

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
