# tekla-reader

Experimental clean-room Tekla Structures DB1 geometry reader. Extracted from OpenAEC Foundation Open 3D Viewer Studio.

```rust,no_run
use tekla_reader::TeklaLoader;
use openaec_model::{load_with_loader, Progress, Source};
let source = Source::from(std::path::Path::new("model.db1"));
let model = load_with_loader(&TeklaLoader, &source, &Progress::default())?;
# Ok::<(), String>(())
```

Readers accept files or `Source::Bytes` (including browser-picked files). Output uses
the shared `openaec-model` types, with no UI, iced, or GPU dependency.

## Validation

`cargo test` runs the self-contained tests. `cargo check --target wasm32-unknown-unknown` checks browser support.

## Current coverage and limitations

Experimental clean-room implementation from `tekla-cleanroom/spec/SPEC_v0.1.md`.
Only DB header versions 8.95 (TS 2020) and 7.82 (TS 19.0). Profile parts use approximate
boxes inferred from profile names. Contour plates are extruded polygons. Offsets, end offsets
and rotation are ignored. Coordinates convert millimetres to metres, Z up; one layer per class.
Self-contained synthetic database fixtures exercise both supported versions.
The external specification is not bundled and is not required to build or test.

## Use in another application

Add this repository as a Cargo Git dependency, pinned to a commit. Also depend on
`openaec-model` from the same Git URL and revision as this crate (see Cargo.toml)
when constructing `Source` or consuming scene types. These repositories are private;
Git needs an account or deploy key with read access.
