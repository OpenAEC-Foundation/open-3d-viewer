# Vendored crates

SKP, Tekla, PLY and their shared model are standalone Git dependencies in Cargo.toml.

`openaec-ui/` is a snapshot of `crates/openaec-ui` from the
[OpenAEC Style Repo](https://github.com/OpenAEC-Foundation/OpenAEC-Style-Repo), commit `6c9c896`
(28 October 2026, crate version 2026.44.1, iced 0.14). The Style Repo is the source of truth;
Open Pointcloud Studio carries a snapshot the same way. Local changes, for the web build (wasm32):
`thread-pool` only off the web (`Cargo.toml`), and no minimize / maximize / close in the title bar
in the browser (`src/shell.rs`). To update, copy the
crate again from the Style Repo and run its `scripts/check_portable_ui.py` there first.
Licenses: see `openaec-ui/LICENSE.md` and `openaec-ui/NOTICE.md`.
