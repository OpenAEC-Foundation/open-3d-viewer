# ply-reader

Rust PLY mesh reader with ASCII and binary little/big endian support. Extracted from OpenAEC Foundation Open 3D Viewer Studio.

```rust,no_run
use ply_reader::PlyLoader;
use openaec_model::{load_with_loader, Progress, Source};
let source = Source::from(std::path::Path::new("model.ply"));
let model = load_with_loader(&PlyLoader, &source, &Progress::default())?;
# Ok::<(), String>(())
```

Readers accept files or `Source::Bytes` (including browser-picked files). Output uses
the shared `openaec-model` types, with no UI, iced, or GPU dependency.

## Validation

`cargo test` runs the self-contained tests. `cargo check --target wasm32-unknown-unknown` checks browser support.

## Current coverage and limitations

ASCII, binary little endian and big endian; optional vertex normals and colours; polygon faces use fan triangulation. Z up. Files without faces are rejected; this is a mesh reader, not a point-cloud reader.

## Use in another application

Add this repository as a Cargo Git dependency, pinned to a commit. Also depend on
`openaec-model` from the same Git URL and revision as this crate (see Cargo.toml)
when constructing `Source` or consuming scene types. These repositories are private;
Git needs an account or deploy key with read access.
