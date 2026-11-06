# Vendored crates

SKP, Tekla and PLY readers are standalone Git dependencies in Cargo.toml.
The viewer's scene model and clipping code live in the app itself.

`openaec-ui/` is a snapshot of `crates/openaec-ui` from the
[OpenAEC Style Repo](https://github.com/OpenAEC-Foundation/OpenAEC-Style-Repo), commit `6c9c896`
(28 October 2026, crate version 2026.44.1, iced 0.14). The Style Repo is the source of truth;
Open Pointcloud Studio carries a snapshot the same way. Local changes, for the web build (wasm32):
`thread-pool` only off the web (`Cargo.toml`), and no minimize / maximize / close in the title bar
in the browser (`src/shell.rs`). To update, copy the
crate again from the Style Repo and run its `scripts/check_portable_ui.py` there first.
Licenses: see `openaec-ui/LICENSE.md` and `openaec-ui/NOTICE.md`.

`vertex/` holds the Vertex BD / BTL reader as compiled shared libraries (`vertex.dll`,
`libvertex.so`, `libvertex.dylib`) from a release of the private OpenAEC repository `vertex.dll`;
its source and sample projects stay there. The viewer loads the library at run time and the
installers ship it next to the program. To update: `scripts/update-vertex.sh <tag>`; the version
and checksums are in `vertex/README.md` and `vertex/SHA256SUMS`.
