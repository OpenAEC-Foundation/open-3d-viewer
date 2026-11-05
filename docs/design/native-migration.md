# Native migration capability ledger

The native validator and viewer share a Rust source tree. This ledger distinguishes implemented code from release acceptance; an implementation entry alone is not a parity claim.

| Capability | Native implementation | Remaining acceptance or limitation |
|---|---|---|
| Common 3D renderer and model tools | Existing iced/wgpu viewer | Validate both product builds and native UI |
| Separate products | Compile-time product descriptor, distinct executable/settings/install IDs | Separate signed workflow implemented; credentialed execution and publication remain pending |
| IFC semantic data | IFClite core 25.0.1, including entities without geometry | Full IFC2X3/IFC4 conformance is not claimed |
| IDS validation | Native `bim-validator-core`, typed IDS input | 307 official paired cases: 292 exact matches, 14 not checkable, 1 invalid input; see core README for precise scope |
| Background validation | Worker, cancellation and generation guard | Cancellation is cooperative |
| Finding selection | Source-scoped GlobalId lookup in loaded geometry | Entities without geometry remain reportable without selectable meshes |
| Reports | JSON and CSV, same core through GUI and CLI | No HTML/PDF report renderer yet |
| BCF 2.1 | Import, status edit, archive preservation, findings export | Full XSD validation of imports is not implemented |
| BCF viewpoints | Camera and metadata read, original files preserved | Viewer does not yet restore camera, visibility, clipping or coloring; selection frames a component |
| BCF creation | Topic and component references | No viewport snapshot capture yet |
| HTTP API and hosted jobs | Existing service remains available separately | Not migrated to native Rust |
| Browser validation | Existing web validator remains separate | New native validation interface is not a WASM target |
| Performance | Existing native renderer reused | No comparative validation benchmark published |

No Python process, service or embedded interpreter is invoked by native validation. Python used by packaging or development-only schema checks is not an application dependency.

## Local verification

```sh
cargo test -p bim-validator-core
cargo test -p bim-bcf
cargo test --workspace --features bim-validator
cargo check --no-default-features --bin open-3d-viewer
python -m unittest discover -s scripts -p test_build_products.py
```

Release acceptance requires review of corpus mismatches, both desktop builds, installer checks, signing verification and native UI evidence. The pre-existing validator release remains the supported validation distribution until those checks succeed.
