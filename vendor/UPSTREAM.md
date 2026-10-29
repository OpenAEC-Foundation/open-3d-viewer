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

Viewer UI overlay: the prototype `Theme` roles map onto the existing generated
`design::tokens` UI 0.4 palette from OpenAEC-Blueprint (`design/ontwerp-0.4/tokens-0.4.json`).
The generated token files are unchanged. `ribbon_body` lays out actions in compact
three-row columns and is shared by the normal shell and the headless viewer preview.
The app supplies one Model ribbon with file, display, camera, section, explode,
projection and measurement commands. These changes are local to this vendored copy.

The File tab uses the existing anchored popover for Open, Save as and Settings.
The general settings dialog shows language, theme and interface size together.
Popover widget operations forward to their contents for accessibility and UI tests.
`application_view` shares the runtime's actual shell with interaction tests and
the viewer's `--screenshot` command, including File and Settings previews.
