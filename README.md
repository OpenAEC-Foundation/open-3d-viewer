# Open 3D Viewer Studio

A 3D model viewer for AEC files, written in Rust: the UI is [iced](https://iced.rs) 0.14 and the
3D viewport is a custom wgpu pipeline inside iced's `shader` widget. One code base runs as a
desktop app (Windows, Linux, macOS) and as a web app (wasm32 on WebGPU) at
[open-3d-viewer.open-aec.com](https://open-3d-viewer.open-aec.com). No web view, no JavaScript
framework.

## Repository layout

| Path | What |
| --- | --- |
| `src/lib.rs` | the viewer (app state, viewport, loaders, writers, measure, section, explode) |
| `src/main.rs` | entry point: [`desktop`](src/desktop.rs) natively, [`web`](src/web.rs) in the browser |
| `src/bin/open-3d-viewer-mcp.rs` | MCP server for AI agents (desktop) |
| `index.html`, `Trunk.toml` | the web app page, built with [trunk](https://trunkrs.dev) |
| `revit/` | the desktop app with Revit support (private `rvt-reader`, see below) |
| `vendor/openaec-ui/` | shared OpenAEC window shell, tokens, widgets and icons (see `vendor/UPSTREAM.md`) |
| `assets/` | fonts, app icon and logo |
| `test-models/` | test and sample models |

## Build and run

Needs a Rust toolchain (1.88+).

```sh
cargo run --release -- path/to/model.skp    # desktop app, optionally opening a file
cargo test --release                        # loader, camera, parser and writer tests
```

Web app (needs `rustup target add wasm32-unknown-unknown` and `cargo install --locked trunk`):

```sh
trunk serve --release      # http://127.0.0.1:8080
trunk build --release      # into dist/, as deployed
```

The web app needs a browser with WebGPU (current Chrome, Edge, Firefox or Safari). Files open
through the file picker, by dropping them onto the page or with a link:
`?file=https://example.org/model.glb`. Save as downloads the file.

### Revit

Revit support comes from the private OpenAEC `rvt-reader`
([OpenAEC-Foundation/rvt-dll](https://github.com/OpenAEC-Foundation/rvt-dll)), so it is not in the
main package: `revit/` builds the same desktop app with the Revit loader added. It expects the
rvt-reader checkout next to this repository (`../rvt-reader`).

```sh
cd revit
cargo build --release      # revit/target/release/open-3d-viewer(.exe)
```

Revit files open in the desktop app only (the reader opens the file itself).

### Command line

```
open-3d-viewer [FILE] [--theme light|forge|night|blueprint] [--no-remote]
open-3d-viewer [FILE] --screenshot OUT.png [--size 1600x1000] [--mode solid|wireframe|edges]
               [--view iso|top|front|right] [--ortho] [--view-tab] [--theme ...]
               [--explode X,Y,Z (percent)] [--explode-mode storeys|elements]
open-3d-viewer [FILE] --window-screenshot OUT.png
open-3d-viewer --bench FILE [--frames 300]
open-3d-viewer FILE --save-as OUT.ifc      (convert without a window)
```

* `--screenshot` renders one frame of the whole UI headlessly (iced_test simulator on an
  offscreen wgpu device, 2x scale) and writes a PNG; no window is opened.
* `--window-screenshot` opens the real window, waits until the model is shown, captures the
  window contents through iced (not the screen) and exits.
* `--bench` loads a model and renders frames of the viewport pipeline offscreen at 1920x1080
  with 4x MSAA, printing load time and frame times.

## What works

* **OpenAEC shell** (`openaec-ui`): title strip, ribbon with the File backstage, explorer and
  properties panes, status bar, settings dialog (theme, language, scale) and the OpenAEC themes.
* **Viewport** (wgpu, 4x MSAA, depth buffer): Z-up scene in metres, orbit (left drag), pan
  (right/middle drag or Shift+left), zoom (wheel), fit on load, view cube, ground grid with
  axis lines, two-sided lighting, per-layer and per-vertex colours, layer visibility, render
  modes Solid, Wireframe and Edges, view presets Iso / Top / Front / Right, perspective or
  orthographic projection, "Flip up" for files with the other up-axis convention.
* **Tools**: picking with element properties, section box and section plane (with cut fills),
  exploded view per storey or per element, measuring (distance, angle, polygon, area), saved
  views and grids from Revit models.
* **Open**: file dialog (Ctrl+O), command line, drag and drop; loading runs on a worker thread
  on the desktop.
* **Keyboard**: Ctrl+O open, F fit, G grid, S/W/E solid/wireframe/edges, 1/2/3/0
  top/front/right/iso, P/O perspective/orthographic.

### Formats

| Format | Crate | Notes |
| --- | --- | --- |
| Revit `.rvt` / `.rfa` | `rvt-read` + `rvt-export` (OpenAEC, `revit/` build only) | one layer per category batch, Y up → Z up; levels, grids, rooms and views |
| SketchUp `.skp` | [`skp-reader`](https://github.com/OpenAEC-Foundation/skp-reader) | existing partial openskp port; 2021+ ZIP container and classic MFC archive (SketchUp 3 to 2020); tags, material colours, pickable groups/components and dynamic attributes; inches → metres, Z up |
| Tekla Structures `.db1` (beta) | [`tekla-reader`](https://github.com/OpenAEC-Foundation/tekla-reader) | clean-room reader; database versions 8.95 (TS 2020) and 7.82 (TS 19.0). Profile parts as approximate boxes, contour plates as extruded polygons; offsets and rotation ignored; mm → metres, Z up |
| glTF `.gltf` / `.glb` | `gltf` | default scene flattened, one layer per material, each node with a mesh a pickable element, base colour × average texture colour, vertex colours, Y up → Z up. In the browser: `.glb` or `.gltf` with embedded buffers |
| Wavefront `.obj` | `tobj` | one layer per object/group, MTL diffuse colours (desktop), vertex colours; treated as Y up |
| STL `.stl` | `stl_io` | ASCII and binary, flat shaded, kept Z up |
| PLY `.ply` | [`ply-reader`](https://github.com/OpenAEC-Foundation/ply-reader) | ASCII / binary LE / BE, vertex normals and colours, polygons fan-triangulated; kept Z up |

The extracted readers share [`openaec-model`](https://github.com/OpenAEC-Foundation/openaec-model):
scene types, mesh utilities, saved views, clipping geometry and the `Loader` / `Source` contract,
without iced or GPU dependencies. The viewer re-exports these types through its existing API,
so the Revit adapter keeps working with the same types. Revit already uses the existing
`rvt-read`, `rvt-export` and `rvt-model` crates; glTF, OBJ and STL reuse `gltf`, `tobj` and `stl_io`.

All four extracted repositories are private, matching this repository. This viewer consumes
pinned source snapshots under `vendor/`; see [vendor/FORMATS.md](vendor/FORMATS.md) for revisions,
update instructions and standalone crate/corpus tests. Other applications can depend directly
on the reader repositories with Cargo Git dependencies.

New formats: implement `loaders::Loader` (name, extensions, `load` from a `loaders::Source`, a
file or bytes, → `Vec<Layer>` in Z up, optional `source_y_up`) in a new module and add it to
`loaders::registry()`. Loaders in other crates are added with `loaders::add` before the app
starts (as `revit/` does).

### Save as

File → Save as (Ctrl+Shift+S) writes the open model in another format; on the web the file is
downloaded.

| Format | Notes |
| --- | --- |
| IFC 4 `.ifc` (ADD2 TC1, Reference View) | own writer, any model the viewer opens. Project, site, building and one storey per Revit level (else one storey); each element in its level, or the storey it stands on. One IFC element per model element, one per layer for geometry outside elements. Class from the Revit category or the Tekla part name, else IfcBuildingElementProxy. Body: IfcTriangulatedFaceSet per colour, welded, `Closed` for closed shells, one shared IfcSurfaceStyle per colour. Properties as a property set named after the source format, materials as IfcMaterial / IfcMaterialConstituentSet, layers as IfcPresentationLayerAssignment. GlobalIds stable across saves. Metres; OBJ, STL and PLY larger than 500 units are written in millimetres |

Checked with IfcOpenShell 0.8 (schema and where rules, spatial containment, unique GlobalIds,
geometry): Revit samples (456 and 6773 elements), 11 SketchUp test models, OBJ, STL and glTF.

New writers: implement `export::Writer` (name, extensions, `write` to any `io::Write`) in a new
module and add it to `export::registry()`.

## AI agents (MCP server)

`cargo build --release` also builds `open-3d-viewer-mcp`, an [MCP](https://modelcontextprotocol.io)
server through which an AI agent (Claude Code, Claude Desktop or any MCP client) drives the
desktop viewer as a person does: open files, take screenshots, orbit / pan / zoom and set the
camera, standard and saved views, show / hide / isolate layers, find elements by property,
select, inspect properties, section box and section plane, render mode and display toggles, and
measure. Measurements are drawn in the viewport (View > Measure > Clear measurements removes
them).

Install: keep `open-3d-viewer-mcp` next to the viewer and register it with the client:

```sh
claude mcp add open-3d-viewer -- "C:\path\to\open-3d-viewer-mcp.exe"   # Claude Code
open-3d-viewer-mcp --print-config        # the JSON for claude_desktop_config.json and others
```

How it works: the window listens on a free port on 127.0.0.1 and writes the port and a random
token to `%APPDATA%\OpenAEC\Open 3D Viewer Studio\remote.json`; commands go through the same
update path as clicks. The MCP server connects to that viewer and starts one when none runs
(`O3V_VIEWER`, else `open-3d-viewer` next to it). `--no-remote` or `O3V_REMOTE=0` switches the
port off; `O3V_REMOTE_PORT` fixes the port.

## Performance (Revit sample, this machine)

`racbasicsampleproject_2026.rvt` (18.8 MB), AMD Radeon integrated GPU (Vulkan), release build:
load + mesh 2.3-2.6 s, 17 layers, about 53-59 thousand triangles. `--bench` at 1920x1080 with 4x
MSAA: first frame including GPU upload ~50 ms, then 8.7-8.9 ms per frame (~112 fps) in Solid and
Edges mode.

## Not there yet

* **IFC import**: there is no mature pure-Rust IFC geometry kernel yet; IFC export is native.
* **More formats**: DWG, DXF, E57, LAS and other point clouds, FBX, 3DS, Collada, 3MF, AMF,
  VRML, VTK, KMZ, VOX, G-code, STEP/IGES/BREP, XYZ/PCD. PLY files without faces are rejected.
* **Textures**: glTF/OBJ/SketchUp textures are not mapped (an average tint instead); SketchUp
  soft edges are not smoothed.
* **Web**: no Revit, preferences are not kept between visits, loading blocks the page while it
  runs (no worker threads yet).
* Walk mode, points and hidden-line render modes, recent files, multiple models at once.

## Licences

The app is LGPL-3.0-or-later. Dependencies: iced and iced_test (MIT), wgpu (MIT OR Apache-2.0,
via iced), glam (MIT OR Apache-2.0), bytemuck (Zlib OR Apache-2.0 OR MIT), rfd (MIT), gltf
(MIT OR Apache-2.0), tobj (MIT), stl_io (MIT), web-time (MIT OR Apache-2.0), wasm-bindgen and
web-sys (MIT OR Apache-2.0). Inter font: SIL OFL 1.1 (`assets/fonts/Inter-LICENSE.txt`). Theme
colours: OpenAEC style book (CC BY-SA 4.0). `vendor/openaec-ui`: see its `NOTICE.md`.

`rvt-reader` (OpenAEC) is a private crate with a proprietary licence. It is linked only into the
`revit/` build, never into the main package or the web app.
