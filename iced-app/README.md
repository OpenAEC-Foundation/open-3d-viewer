# Open 3D Viewer Studio — Rust + iced edition

A pure-Rust rewrite of the Open 3D Viewer Studio desktop app: the UI is [iced](https://iced.rs)
0.14 and the 3D viewport is a custom wgpu pipeline inside iced's `shader` widget. No web view,
no JavaScript. It lives next to the Tauri/React app (`../src`, `../src-tauri`), which stays the
reference.

## Build and run

Needs a Rust toolchain (1.88+) and, for Revit support, the private OpenAEC `rvt-reader`
checkout next to this repository (`../../rvt-reader`, same as `src-tauri/Cargo.toml`).

```sh
cd iced-app
cargo build --release                 # with Revit support (default feature "rvt")
cargo build --release --no-default-features   # without rvt-reader
cargo run --release -- path/to/model.glb      # open a file on start
cargo test --release                  # loader, camera and parser tests
```

Command line:

```
open-3d-viewer-iced [FILE] [--theme light|forge|night|blueprint] [--no-remote]
open-3d-viewer-iced [FILE] --screenshot OUT.png [--size 1600x1000] [--mode solid|wireframe|edges]
                    [--view iso|top|front|right] [--ortho] [--view-tab] [--theme ...]
open-3d-viewer-iced [FILE] --window-screenshot OUT.png
open-3d-viewer-iced --bench FILE [--frames 300]
open-3d-viewer-iced FILE --save-as OUT.ifc      (convert without a window)
```

* `--screenshot` renders one frame of the whole UI headlessly (iced_test simulator on an
  offscreen wgpu device, 2x scale) and writes a PNG; no window is opened.
* `--window-screenshot` opens the real window, waits until the model is shown, captures the
  window contents through iced (not the screen) and exits.
* `--bench` loads a model and renders frames of the viewport pipeline offscreen at 1920x1080
  with 4x MSAA, printing load time and frame times.

## What works

* **Layout as in the Tauri app**: title/tab bar with Open, Home/View ribbon tabs with icon
  buttons, layers panel left, 3D viewport centre, properties panel right, status bar bottom.
* **OpenAEC themes** from `src/styles/openaec-themes.css`: Light, Forge, Night (openaec) and
  Blueprint, Inter font (bundled, OFL).
* **Viewport** (wgpu, 4x MSAA, depth buffer): Z-up scene in metres, orbit (left drag), pan
  (right/middle drag or Shift+left), zoom (wheel), fit on load, ground grid with major lines and
  red/green X/Y axis lines, two-sided Lambert lighting (key + fill + sky + head light),
  per-layer colours and per-vertex colours, layer visibility, render modes Solid, Wireframe (all
  triangle edges) and Edges (solid + crease/border edges), view presets Iso / Top / Front /
  Right, perspective or orthographic projection, "Flip up" to fix files with the other up-axis
  convention. Layers too big for one GPU buffer are split into parts on load.
* **Layers panel**: one row per layer (visibility toggle, colour swatch, triangle count),
  filter box, show all / hide all (applies to the filtered rows).
* **Properties panel**: file name, format, size, folder, load time, layer/vertex/triangle
  counts, visible triangles, bounds (min/max/size) and up axis.
* **Open**: native file dialog (`rfd`), command-line argument, drag and drop onto the window,
  Ctrl+O. Loading runs on a worker thread.
* **Keyboard**: Ctrl+O open, F fit, G grid, S/W/E solid/wireframe/edges, 1/2/3/0
  top/front/right/iso, P/O perspective/orthographic.

### Formats

| Format | Crate | Notes |
| --- | --- | --- |
| Revit `.rvt` / `.rfa` | `rvt-read` + `rvt-export` (OpenAEC, path dependency) | one layer per category batch, Y up → Z up |
| SketchUp `.skp` | own port of openskp 1.3 (MIT) + `flate2`, `earcutr` | 2021+ ZIP container and 2013-2020 MFC archive; one layer per tag (hidden tags start off), material colours per vertex (textures as their average colour), each component/group a pickable element with its dynamic attributes; inches → metres, Z up. Files openskp cannot read fail here too |
| Tekla Structures `.db1` (beta) | own clean-room reader + `flate2`, `earcutr` | written only from `tekla-cleanroom/spec/SPEC_v0.1.md` (draft; open questions in `spec/QUESTIONS.md`); database versions 8.95 (TS 2020) and 7.82 (TS 19.0). Profile parts as boxes along their axis in the local frame with the y/z alignment, contour plates (7.82) as their polygon extruded by the thickness; one layer per class, each part a pickable element with name, profile, material and class. Approximation: profile sizes are not in the spec yet, so the box is guessed from the profile name (`PL200*10`, `IPE270` → 270 × 135, else 100 × 100); offsets and rotation are ignored. mm → metres, Z up |
| glTF `.gltf` / `.glb` | `gltf` | default scene flattened, one layer per material, each node with a mesh a pickable element with its name, base colour × average texture colour, vertex colours, Y up → Z up |
| Wavefront `.obj` | `tobj` | one layer per object/group, MTL diffuse colours, vertex colours; treated as Y up |
| STL `.stl` | `stl_io` | ASCII and binary, flat shaded, kept Z up |
| PLY `.ply` | own parser | ASCII / binary LE / BE, vertex normals and colours, polygons fan-triangulated; kept Z up |

New formats: implement `loaders::Loader` (name, extensions, `load` → `Vec<Layer>` in Z up,
optional `source_y_up`) in a new module and add it to `loaders::registry()`.

### Save as

File → Save as (Ctrl+Shift+S) writes the open model in another format; the save dialog has one
filter per writer.

| Format | Notes |
| --- | --- |
| IFC 4 `.ifc` (ADD2 TC1, Reference View) | own writer, any model the viewer opens. Project, site, building and one storey per Revit level (else one storey); each element in its level, or the storey it stands on. One IFC element per model element (Revit elements, Tekla parts, SketchUp components, glTF nodes), one per layer for geometry outside elements. Class from the Revit category (IfcWall, IfcSlab, IfcDoor, IfcWindow, IfcColumn, IfcBeam, IfcDuctSegment, IfcPipeSegment, ...) or the Tekla part name (IfcColumn, IfcBeam, IfcPlate, IfcMember), else IfcBuildingElementProxy. Body: IfcTriangulatedFaceSet per colour relative to the element's placement, welded, degenerate triangles dropped, `Closed` set for closed shells, one shared IfcSurfaceStyle per colour. Properties as a property set named after the source format, materials as IfcMaterial / IfcMaterialConstituentSet, layers as IfcPresentationLayerAssignment. GlobalIds derived from the file name and element id (stable across saves). Metres; OBJ, STL and PLY have no unit, and such a model larger than 500 units is written in millimetres |

Checked with IfcOpenShell 0.8 (schema and where rules, spatial containment, unique GlobalIds,
geometry of every element or a sample of 200): `racbasicsampleproject_2026.rvt` (456 elements,
6 storeys), `rmebasicsampleproject_2022.rvt` (6773 elements, 4 storeys), 11 SketchUp test
models, OBJ, STL and a three-node glTF, all without issues.

New writers: implement `export::Writer` (name, extensions, `save`) in a new module and add it to
`export::registry()`.

## AI agents (MCP server)

`cargo build --release` also builds `open-3d-viewer-mcp.exe`, an [MCP](https://modelcontextprotocol.io)
server through which an AI agent (Claude Code, Claude Desktop or any MCP client) drives the
viewer as a person does: open files, take screenshots, orbit / pan / zoom and set the camera,
standard and saved views, show / hide / isolate layers, find elements by property (and count
them per level, type, …), select, inspect properties, section box and section plane, render
mode and display toggles, and measure (distance and angle between points or clicked pixels,
element size / surface area / volume, clearance between two elements). Measurements are drawn
in the viewport (View > Measure > Clear measurements removes them).

Install: keep `open-3d-viewer-mcp.exe` next to the viewer `.exe` and register it with the
client:

```sh
claude mcp add open-3d-viewer -- "C:\path\to\open-3d-viewer-mcp.exe"   # Claude Code
open-3d-viewer-mcp --print-config        # the JSON for claude_desktop_config.json and others
```

How it works: the window listens on a free port on 127.0.0.1 and writes the port and a random
token to `%APPDATA%\OpenAEC\Open 3D Viewer Studio\remote.json`; commands go through the same
update path as clicks. The MCP server connects to that viewer and starts one when none runs
(`O3V_VIEWER`, else the viewer next to it). `--no-remote` or `O3V_REMOTE=0` switches the port
off; `O3V_REMOTE_PORT` fixes the port.

## Performance (Revit sample, this machine)

`racbasicsampleproject_2026.rvt` (18.8 MB), AMD Radeon integrated GPU (Vulkan), release build:
load + mesh 2.3-2.6 s, 17 layers, about 53-59 thousand triangles (the count follows the
`rvt-reader` version). `--bench` at 1920x1080 with 4x MSAA: first frame including GPU upload
~50 ms, then 8.7-8.9 ms per frame (~112 fps) in Solid and Edges mode, measured with a full GPU
wait after every frame.

## Gaps compared with the Tauri/React app

* **IFC import**: the React app opens IFC with web-ifc (C++/WASM); there is no mature pure-Rust
  IFC geometry kernel yet. IFC export (Save as) is native, see above.
* **SketchUp `.skp`**: no textures (average colour instead) and no soft-edge smoothing, so
  curved surfaces look faceted.
* **DWG, DXF, E57, LAS/point clouds**: not ported (DWG/E57 are being added to the Tauri app
  separately; they can be added here as `Loader`s). PLY files without faces (point clouds)
  are rejected.
* **Other three.js / Assimp / OCCT formats**: FBX, 3DS, Collada, 3MF, AMF, VRML, VTK, KMZ,
  VOX, G-code, STEP/IGES/BREP (OCCT WASM), XYZ/PCD.
* **Textures**: glTF/OBJ textures are not mapped (only an average tint for glTF).
* **Tools**: no interactive measure tool (measuring goes through the MCP server), walk mode, view cube / axis gizmo, zoom to cursor, points and
  hidden-line render modes, light toggles, settings dialog, backstage (recent files), feedback
  dialog, i18n (UI is English only), multiple models at once.
* Layer rows are grouped by Revit category / glTF material / OBJ object rather than the
  React app's scene-graph groups.

## Licences

The app is LGPL-3.0-or-later. Dependencies: iced and iced_test (MIT), wgpu (MIT OR Apache-2.0,
via iced), glam (MIT OR Apache-2.0), bytemuck (Zlib OR Apache-2.0 OR MIT), rfd (MIT), gltf
(MIT OR Apache-2.0), tobj (MIT), stl_io (MIT). Inter font: SIL OFL 1.1
(`assets/fonts/Inter-LICENSE.txt`). Theme colours: OpenAEC style book (CC BY-SA 4.0).

`rvt-reader` (OpenAEC) is a private crate with a proprietary licence
(`LicenseRef-Proprietary`). It is linked only through the optional default feature `rvt`, the
same way as in the Tauri app; `--no-default-features` gives a binary without it.
