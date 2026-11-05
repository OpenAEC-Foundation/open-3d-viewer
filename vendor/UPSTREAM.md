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

The native validation panel extends `design::widgets::button` to accept owned or borrowed labels (`Cow<str>`). This retains the shared button style, focus behavior and accessible name while supporting generated finding labels. Preserve this compatibility change when refreshing the snapshot until it is available upstream.

Local test-only adjustment in `src/frame.rs` selects the software fallback renderer when `gpu-renderer` is enabled, while retaining standalone CPU-only test support.
