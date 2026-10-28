# skp-reader

Partial pure-Rust SketchUp SKP reader, classic and 2021+ containers. Extracted from OpenAEC Foundation Open 3D Viewer Studio.

```rust,no_run
use skp_reader::SkpLoader;
use openaec_model::{load_with_loader, Progress, Source};
let source = Source::from(std::path::Path::new("model.skp"));
let model = load_with_loader(&SkpLoader, &source, &Progress::default())?;
# Ok::<(), String>(())
```

Readers accept files or `Source::Bytes` (including browser-picked files). Output uses
the shared `openaec-model` types, with no UI, iced, or GPU dependency.

## Validation

`cargo test` runs the self-contained tests. `cargo check --target wasm32-unknown-unknown` checks browser support.

## Current coverage and limitations

Partial reader ported from openskp 1.3/1.4.0: classic SketchUp 3–2020 MFC archives and
2021+ ZIP/VFF containers, triangulated faces, placed groups/components, tags, hidden tags,
material colours and dynamic attributes. Inches become metres, Z stays up.
Textures use their average colour; texture mapping and soft-edge smoothing are not implemented.
This is the existing viewer implementation, not a claim of complete SKP compatibility.

Large corpus fixtures stay in the viewer repository. To run its 21-file regression suite,
set `SKP_TEST_MODELS` to the absolute `test-models` directory of open-3d-viewer and run
`cargo test --features corpus-tests`. `skp_dir_survey` is an ignored diagnostic that uses `SKP_DIR`.

## Attribution

The reader is a Rust port of [openskp](https://github.com/iamahsanmehmood/openskp),
MIT, copyright Ahsan Mehmood. Preserve the upstream MIT license in THIRD_PARTY_LICENSES.md.
No sample models are distributed with this crate.

## Use in another application

Add this repository as a Cargo Git dependency, pinned to a commit. Also depend on
`openaec-model` from the same Git URL and revision as this crate (see Cargo.toml)
when constructing `Source` or consuming scene types. These repositories are private;
Git needs an account or deploy key with read access.
