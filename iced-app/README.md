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
open-3d-viewer-iced [FILE] [--theme light|forge|night|blueprint]
open-3d-viewer-iced [FILE] --screenshot OUT.png [--size 1600x1000] [--mode solid|wireframe|edges]
                    [--view iso|top|front|right] [--ortho] [--view-tab] [--theme ...]
open-3d-viewer-iced [FILE] --window-screenshot OUT.png
open-3d-viewer-iced --bench FILE [--frames 300]
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
| glTF `.gltf` / `.glb` | `gltf` | default scene flattened, one layer per material, base colour × average texture colour, vertex colours, Y up → Z up |
| Wavefront `.obj` | `tobj` | one layer per object/group, MTL diffuse colours, vertex colours; treated as Y up |
| STL `.stl` | `stl_io` | ASCII and binary, flat shaded, kept Z up |
| PLY `.ply` | own parser | ASCII / binary LE / BE, vertex normals and colours, polygons fan-triangulated; kept Z up |

New formats: implement `loaders::Loader` (name, extensions, `load` → `Vec<Layer>` in Z up,
optional `source_y_up`) in a new module and add it to `loaders::registry()`.

## Performance (Revit sample, this machine)

`racbasicsampleproject_2026.rvt` (18.8 MB), AMD Radeon integrated GPU (Vulkan), release build:
load + mesh 2.3-2.6 s, 17 layers, about 53-59 thousand triangles (the count follows the
`rvt-reader` version). `--bench` at 1920x1080 with 4x MSAA: first frame including GPU upload
~50 ms, then 8.7-8.9 ms per frame (~112 fps) in Solid and Edges mode, measured with a full GPU
wait after every frame.

## Gaps compared with the Tauri/React app

* **IFC**: the React app uses web-ifc (C++/WASM); there is no mature pure-Rust IFC
  geometry kernel yet.
* **SketchUp `.skp`**: the React app uses the JS `openskp` reader; there is no Rust reader.
* **DWG, DXF, E57, LAS/point clouds**: not ported (DWG/E57 are being added to the Tauri app
  separately; they can be added here as `Loader`s). PLY files without faces (point clouds)
  are rejected.
* **Other three.js / Assimp / OCCT formats**: FBX, 3DS, Collada, 3MF, AMF, VRML, VTK, KMZ,
  VOX, G-code, STEP/IGES/BREP (OCCT WASM), XYZ/PCD.
* **Textures**: glTF/OBJ textures are not mapped (only an average tint for glTF).
* **Tools**: no picking/selection with element properties, section box, measurement
  (distance/angle), walk mode, view cube / axis gizmo, zoom to cursor, points and
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
