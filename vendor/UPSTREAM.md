# Vendored crates

SKP, Tekla and PLY readers are standalone Git dependencies in Cargo.toml.
The viewer's scene model and clipping code live in the app itself.

`openaec-ui/` is a snapshot of `crates/openaec-ui` from
[OpenAEC Blueprint](https://github.com/OpenAEC-Foundation/OpenAEC-Blueprint) (the Style Repo under
its new name), commit `5e4b0f3` (November 2026, crate version 2026.45.0, iced 0.14). Blueprint is
the source of truth; Open Pointcloud Studio carries a snapshot the same way. Local changes, for
the web build (wasm32): `thread-pool` only off the web (`Cargo.toml`), and no minimize / maximize
/ close in the title bar in the browser (`src/shell.rs`); and eight more languages in `Language`
(`src/locale.rs`: German, French, Spanish, Italian, Portuguese, Polish, Swedish and Turkish):
`text(nl, en)` looks those up by the English text in a catalogue the app installs with
`install_catalogue`, falling back to English (the viewer's catalogues are `locales/*.json`, read
by `src/i18n.rs`). Blueprint should take the same change, so other OpenAEC tools get the
languages too. To update, copy the crate again from Blueprint, run its
`scripts/check_portable_ui.py` there first, and apply these three changes again.
Licenses: see `openaec-ui/LICENSE.md` and `openaec-ui/NOTICE.md`.

`vertex/` holds the Vertex BD / BTL reader as compiled shared libraries (`vertex.dll`,
`libvertex.so`, `libvertex.dylib`) from a release of the private OpenAEC repository `vertex.dll`;
its source and sample projects stay there. The viewer loads the library at run time and the
installers ship it next to the program. To update: `scripts/update-vertex.sh <tag>`; the version
and checksums are in `vertex/README.md` and `vertex/SHA256SUMS`.
