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
| `src/loaders/rvt.rs` | desktop Revit adapter using the independent `rvt-dll` crates |
| `vendor/openaec-ui/` | shared OpenAEC window shell, tokens, widgets and icons (see `vendor/UPSTREAM.md`) |
| `assets/` | fonts, app icon and logo |
| `test-models/` | test and sample models |

## Build and run

Needs a Rust toolchain (1.89+) and Git read access to the private OpenAEC format libraries.

```sh
cargo run --release -- path/to/model.skp    # desktop app, optionally opening a file
cargo test --release                        # loader, camera, parser and writer tests
```

Windows live development, using an installed Rust/C++ toolchain (or the portable
GNU tools in `temp/cargo`, `temp/rustup` and `temp/dev-toolchain/mingw64`):

```powershell
pwsh -File scripts/dev.ps1 -Model path/to/model.rvt
pwsh -File scripts/dev.ps1 -Stop
```

The watcher rebuilds incrementally after source changes and restarts its own viewer
with the same model. A failed build keeps the previous viewer open. Dev windows are
marked `dev`; status and compiler output are in `target/dev/status.json` and
`target/dev/build.log`. `-Once` builds and opens once without watching.

Web app (needs `rustup target add wasm32-unknown-unknown` and `cargo install --locked trunk`):

```sh
trunk serve --release      # http://127.0.0.1:8080
trunk build --release      # into dist/, as deployed
```

The web app needs a browser with WebGPU (current Chrome, Edge, Firefox or Safari). Files open
through the file picker, by dropping them onto the page or with a link:
`?file=https://example.org/model.glb`. Save as downloads the file.

### Revit

The normal desktop application includes Revit support through the independent
[`rvt-dll`](https://github.com/OpenAEC-Foundation/rvt-dll) library (`rvt-read`, `rvt-export`,
`rvt-model`). `.rvt` and `.rfa` are registered at startup and appear in File → Open.

```sh
cargo run --release -- path/to/model.rvt
cargo build --release      # target/release/open-3d-viewer(.exe), including Revit
```

Revit files open in the desktop app only (the reader opens the file itself).
The viewer's own source is LGPL-3.0-or-later; the private Revit reader retains its existing
proprietary license. Desktop binaries include that reader and are not wholly open-source builds.

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
* **Open**: file dialog (Ctrl+O), command line and drag and drop replace the loaded model.
  **+ Add** loads an additional model; the Models list shows every loaded file with its own
  visibility toggle. Each source keeps its element properties, including when source IDs overlap.
  Loading runs on a worker thread on the desktop.
* **Keyboard**: Ctrl+O open, F fit, G grid, S/W/E solid/wireframe/edges, 1/2/3/0
  top/front/right/iso, P/O perspective/orthographic.

### Formats

| Format | Crate | Notes |
| --- | --- | --- |
| Revit `.rvt` / `.rfa` | `rvt-read` + `rvt-export` (OpenAEC, standard desktop build) | one layer per category batch, Y up → Z up; levels, grids, rooms and views |
| IFC `.ifc` | [`ifc-lite-processing`](https://github.com/LTplus-AG/ifc-lite) (desktop) | existing IFClite parsing and geometry pipeline, including placements, units and openings; pickable elements with IFC type, GlobalId and available properties; metres, Z up; georeference anchor retained in project information. Space and spatial-zone volumes start hidden in separate `Spaces:` layers and can be enabled without reimporting. Space-only files keep those layers visible. |
| SketchUp `.skp` | [`skp-reader`](https://github.com/OpenAEC-Foundation/skp-reader) | existing partial openskp port; 2021+ ZIP container and classic MFC archive (SketchUp 3 to 2020); tags, material colours, pickable groups/components and dynamic attributes; inches → metres, Z up |
| Tekla Structures `.db1` (beta) | [`tekla-reader`](https://github.com/OpenAEC-Foundation/tekla-reader) | clean-room reader; database versions 8.95 (TS 2020) and 7.82 (TS 19.0). Profile parts as approximate boxes, contour plates as extruded polygons; offsets and rotation ignored; mm → metres, Z up |
| glTF `.gltf` / `.glb` | `gltf` | default scene flattened, one layer per material, each node with a mesh a pickable element, base colour × average texture colour, vertex colours, Y up → Z up. In the browser: `.glb` or `.gltf` with embedded buffers |
| Wavefront `.obj` | `tobj` | one layer per object/group, MTL diffuse colours (desktop), vertex colours; treated as Y up |
| STL `.stl` | `stl_io` | ASCII and binary, flat shaded, kept Z up |
| PLY `.ply` | [`ply-reader`](https://github.com/OpenAEC-Foundation/ply-reader) | ASCII / binary LE / BE, vertex normals and colours, polygons fan-triangulated; kept Z up |
| CityJSON `.city.json` / `.cityjson`, sequences `.jsonl` / `.cityjsonl` | `serde_json` + `earcutr` | CityJSON 1.0/1.1/2.0 and CityJSONFeature sequences; highest LoD per object; MultiSurface, CompositeSurface, Solid, MultiSolid and CompositeSolid; polygon holes, diffuse material colours and object attributes. No textures, template instances or CRS reprojection. Source axes retained, distances assume metres. |
| LandXML `.xml` / `.landxml` | `roxmltree` | LandXML 1.x TIN surfaces, sparse point IDs, hidden faces, named surfaces and metric/imperial units converted to metres. Survey points and alignments alone are not displayed. |
| DWG `.dwg` / DXF `.dxf` (polygon subset) | [`acadrust`](https://github.com/HakanSeven12/acadrust) | native DWG and ASCII/binary DXF; model-space 3DFACE, MESH, polyface and polygon meshes, including ordinary nested INSERTs; layers, colours and insertion units. ACIS solids, curves, annotations and external references are not rendered; omitted entities are reported in project information. |

CityJSON, LandXML and CAD import retain a double-precision world origin in project
information and display coordinates relative to that origin. Adding files does not
automatically align different origins or reproject coordinate systems. Example files:
`test-models/cityjson-building.city.json` and `test-models/landxml-terrain.xml`.

The app owns its scene model, mesh utilities, saved views, clipping geometry and loader
registry. Each format library returns its own file-reading output; small adapters in
`src/loaders/` convert that output into the app's `Layer` types. No shared model crate is needed.
Revit uses the existing `rvt-read`, `rvt-export` and `rvt-model` crates; glTF, OBJ and STL
reuse `gltf`, `tobj` and `stl_io`.

The three extracted readers have their own private repositories, independent names, documentation and
native/wasm CI. This viewer uses them directly as Cargo Git dependencies, pinned to full commit
hashes in Cargo.toml and Cargo.lock. Parser source is maintained in those repositories.
CI and web deployment use read-only deploy keys stored in FORMAT_CRATES_SSH_KEYS.
Other applications use the same Git dependencies without importing this application.

To update a library, change its revision in Cargo.toml and run `cargo update`.
Each library runs its own tests. The Format crates workflow also runs the SKP corpus
against this repository's test-models directory; the ignored survey uses SKP_DIR.

New formats: implement `loaders::Loader` (name, extensions, `load` from a `loaders::Source`, a
file or bytes, → `Vec<Layer>` in Z up, optional `source_y_up`) in a new module and add it to
`loaders::registry()`. Loaders in other crates are added with `loaders::add` before the app
starts.

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

* **More formats**: full DWG/DXF solids and drawings, CityGML, STEP/IGES, E57, LAS and other point clouds, FBX, 3DS, Collada, 3MF, AMF,
  VRML, VTK, KMZ, VOX, G-code, BREP, XYZ/PCD. PLY files without faces are rejected.
* **Textures**: glTF/OBJ/SketchUp textures are not mapped (an average tint instead); SketchUp
  soft edges are not smoothed.
* **Web**: IFC import and Revit are desktop-only; IFC export works on both targets. Preferences are not kept between visits, loading blocks the page while it
  runs (no worker threads yet).
* Walk mode, points and hidden-line render modes, recent files.

## Licences

The app is LGPL-3.0-or-later. Dependencies: iced and iced_test (MIT), wgpu (MIT OR Apache-2.0,
via iced), glam (MIT OR Apache-2.0), bytemuck (Zlib OR Apache-2.0 OR MIT), rfd (MIT), gltf
(MIT OR Apache-2.0), tobj (MIT), stl_io (MIT), web-time (MIT OR Apache-2.0), wasm-bindgen and
web-sys (MIT OR Apache-2.0). Inter font: SIL OFL 1.1 (`assets/fonts/Inter-LICENSE.txt`). Theme
colours: OpenAEC style book (CC BY-SA 4.0). `vendor/openaec-ui`: see its `NOTICE.md`.
IFClite's independent parsing and geometry libraries are MPL-2.0.
The CAD reader `acadrust` is MPL-2.0; `earcutr` is ISC and `roxmltree` is MIT OR Apache-2.0.

The independent `rvt-dll` crates (OpenAEC) retain their proprietary licence. They are linked
into the standard desktop app, and excluded from the web build.
