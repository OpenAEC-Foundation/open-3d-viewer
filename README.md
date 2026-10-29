**English** · [Nederlands](README.nl.md)

<div align="center">

# Open 3D Viewer Studio

**A 3D viewer for building models that opens SketchUp, Tekla, glTF, OBJ, STL and PLY, measures and sections them, and saves them as IFC, on the desktop and in the browser.**

[![Release](https://img.shields.io/github/v/release/OpenAEC-Foundation/open-3d-viewer?include_prereleases&label=release&color=D97706)](https://github.com/OpenAEC-Foundation/open-3d-viewer/releases/latest)
[![Build](https://github.com/OpenAEC-Foundation/open-3d-viewer/actions/workflows/build.yml/badge.svg)](https://github.com/OpenAEC-Foundation/open-3d-viewer/actions/workflows/build.yml)
[![License](https://img.shields.io/badge/license-LGPL--3.0-blue)](#license)

### [Try in your browser](https://open-3d-viewer.open-aec.com) &nbsp;·&nbsp; [Download](https://github.com/OpenAEC-Foundation/open-3d-viewer/releases/latest)

![A SketchUp model of a house on its plot in the viewer, with the layers on the left and the file properties on the right, theme Light](docs/images/readme/hero.png)

</div>

Open 3D Viewer Studio is a viewer for the 3D models that architects, engineers and builders exchange. It opens a model, lets you walk around it, pick elements and read their properties, measure, cut it open with a section box or plane, pull it apart per storey or per element, and save it as IFC.

It runs in your browser with nothing to install, and as a desktop app on Windows, macOS and Linux. Both come from one Rust code base.

## Try it

**[Open the browser version](https://open-3d-viewer.open-aec.com)**. Drop a model on the page or choose **Open**, then orbit with the left mouse button, pan with the middle button and zoom with the wheel. You need a browser with WebGPU: a current Chrome, Edge, Firefox or Safari. Your file stays on your computer; nothing is uploaded.

## Download

Every release is on the [releases page](https://github.com/OpenAEC-Foundation/open-3d-viewer/releases/latest). The easiest way is the [OpenAEC Installer](https://github.com/OpenAEC-Foundation/openaec-installer): it installs, updates and starts every OpenAEC tool.

| Platform | Files |
|----------|-------|
| **Windows** | `.exe` installer (x64, no administrator rights needed) or `.zip` |
| **macOS** | `.dmg` (universal: Apple silicon and Intel) |
| **Linux** | `.deb`, `.rpm`, `.AppImage` or `.tar.gz` |

When the OpenAEC Installer has a newer version, the title bar of the viewer says so, and **Update** hands over to the installer. Releases are signed: Windows with Authenticode, macOS with a Developer ID and notarized by Apple, Linux with the OpenAEC OpenPGP key, and every file with a build attestation. The release notes say per file whether it is signed; a release without the Windows or macOS signature is marked as a pre-release.

## Why use it?

- **Free and open.** LGPL-3.0. No licence fees, no account, no upload.
- **Opens what you get sent.** SketchUp (version 3 and later), Tekla Structures, glTF, OBJ, STL and PLY, without the program that made them.
- **Saves as IFC.** Any model the viewer opens can be saved as IFC 4, with storeys, classes, properties and materials.
- **The same app in the browser and on the desktop.** One code base; the browser version needs no installation.
- **Measure and cut.** Distances, angles, areas and polygons, a section box and a section plane with filled cuts, and an exploded view.
- **AI agents can drive it.** An MCP server lets Claude and other agents open files, look around, select and measure, in the window you see.

## Screenshots

| | |
|---|---|
| ![The house model in the four themes Light, Dark, Blueprint and High contrast dark](docs/images/readme/themes.png) **Light, Dark, Blueprint and High contrast dark** | ![The plot from above in wireframe with an orthographic camera, theme Dark](docs/images/readme/plan.png) **Top view, wireframe and orthographic** |
| ![A bathroom model pulled apart per element](docs/images/readme/explode.png) **Exploded view per element** | ![The File page with Open, Save as, Settings and the recent files](docs/images/readme/file.png) **The File page with recent files** |
| ![The settings dialog with theme, workspace, status bar, interface size and language, theme Blueprint](docs/images/readme/settings.png) **Settings, theme Blueprint** | ![The shortcuts page in the settings, where every command can get its own key](docs/images/readme/shortcuts.png) **Your own shortcuts for every command** |

## Viewing and navigating

- **Orbit, pan and zoom.** Left drag orbits, middle drag pans, the wheel zooms to the cursor. The view cube turns the model to a side; **F** fits the model in view.
- **Standard views.** Iso, top, front and right (`0`, `1`, `2`, `3`), perspective or orthographic (`P`, `O`), and the saved views of a Revit model.
- **Render modes.** Solid, wireframe and solid with edges (`S`, `W`, `E`), the ground grid (`G`) and **Flip up** for files with the other up axis.
- **Layers and properties.** The explorer lists the layers (SketchUp tags, Tekla classes, glTF materials) with their colour and count; a click on an element shows its properties.
- **Shortcuts.** Every command has one entry in one list, used by the ribbon, the title bar and the keyboard. In **Settings** you give any command your own key.

## Measure, section and explode

- **Measure** distances, angles, polygons and areas by clicking in the model, or between two elements. Measurements stay in the view until you clear them.
- **Section box and section plane** cut the model open, with the cut faces filled. The box can fit the selection.
- **Exploded view** pulls the model apart per storey or per element, by a percentage per axis.

## Revit (separate build)

Revit files open in a separate desktop build, because the Revit reader ([rvt-reader](https://github.com/OpenAEC-Foundation/rvt-dll)) is a private OpenAEC crate. That build shows the levels, grids, rooms, saved views and schedules of the file. A schedule opens in its own tab as a table under the 3D view, and a row selects its elements. Revit keeps only the definition of a schedule in the file, so the viewer computes the rows from the elements; cells it cannot work out yet show `?`.

![A Revit model with its Door Schedule opened as a table under the 3D view, theme Dark](docs/images/readme/schedule.png)

The images on this page are made by [`scripts/build_readme_images.py`](scripts/build_readme_images.py), which renders the real window with `open-3d-viewer --screenshot`. The schedule image uses the Autodesk sample `snowdon_arch_2025.rvt`.

## Interoperability

| Format | Open | Save |
|--------|------|------|
| **IFC 4** (`.ifc`) | No | Yes |
| **SketchUp** (`.skp`, version 3 and later) | Yes | No |
| **Tekla Structures** (`.db1`, beta) | Yes | No |
| **Revit** (`.rvt`, `.rfa`) | Revit build only | No |
| **glTF** (`.gltf`, `.glb`) | Yes | No |
| **Wavefront OBJ** (`.obj`) | Yes | No |
| **STL** (`.stl`, ASCII and binary) | Yes | No |
| **PLY** (`.ply`) | Yes | No |

What does not come through:

- **IFC export** writes each element as triangles (`IfcTriangulatedFaceSet`), not as parametric geometry. Classes come from the Revit category or the Tekla part name, else `IfcBuildingElementProxy`. Checked with IfcOpenShell 0.8 on Revit, SketchUp, OBJ, STL and glTF models.
- **Textures** of glTF, OBJ and SketchUp become their average colour; SketchUp soft edges are not smoothed.
- **Tekla** parts are boxes along their axis and plates are their outline with thickness; profile shapes are not drawn. The reader is written from a clean-room specification.
- **In the browser**, `.gltf` files need their buffers embedded, OBJ materials are not read, and Revit does not open.

New formats: implement `loaders::Loader` and add it to `loaders::registry()`; new writers implement `export::Writer` and go in `export::registry()`.

## MCP

`open-3d-viewer-mcp` is an [MCP](https://modelcontextprotocol.io) server through which an AI agent (Claude Code, Claude Desktop or any MCP client) drives the desktop viewer as a person does: open files, take screenshots, set the camera and the standard and saved views, show, hide and isolate layers, find elements by property, select, read properties, set a section box or plane, change the render mode, read Revit schedules and measure.

- **You see what it does.** Commands go through the same path as your clicks, in the window on your screen. Measurements stay in the view.
- **It stays on your computer.** The window listens on `127.0.0.1` only, with a random token in `%APPDATA%\OpenAEC\Open 3D Viewer Studio\remote.json`. `--no-remote` or `O3V_REMOTE=0` switches it off.
- **Install:** keep `open-3d-viewer-mcp` next to the viewer (the installers do) and register it, for example `claude mcp add open-3d-viewer -- "C:\path\to\open-3d-viewer-mcp.exe"`. `open-3d-viewer-mcp --print-config` prints the JSON for other clients.

## Testing and reliability

- **97 unit tests** in `src/`, plus 2 for the MCP server, run on every pull request and before every release: 24 for the loaders, 13 for the app, 8 for the window shell, 7 for the section tools, 7 for IFC export, 11 for measuring, and more for the camera, view cube, exploded view and remote control.
- **The window shell is tested without a screen.** One test lays out every view and fails when a control has no name for a screen reader; others check that no two commands share a shortcut and that every setting has a name in both languages.
- **Generated files are checked.** `python scripts/build_readme_images.py --check` fails when an image is missing, unused or larger than 150 kB; `python scripts/b2_icons.py <Blueprint> --check` fails when an icon differs from the OpenAEC Blueprint release.
- **IFC export** is checked with IfcOpenShell 0.8 (schema and where rules, spatial containment, unique GlobalIds, geometry) on Revit samples of 456 and 6773 elements, 11 SketchUp test models, OBJ, STL and glTF.

Count sources: `cargo test --release`; the release runs the same checks in [`release.yml`](.github/workflows/release.yml).

## Current limitations

- **IFC does not open yet.** There is no mature IFC geometry kernel in pure Rust; IFC export is native.
- **Many formats are missing.** DWG, DXF, point clouds (E57, LAS), FBX, 3DS, Collada, 3MF, STEP and IGES do not open. PLY files without faces are rejected.
- **No undo yet.** Measurements and the section are cleared by hand.
- **The browser version blocks while it loads** a large model (no worker threads yet) and does not keep your settings between visits.
- **Revit needs the separate build**, which is not in the releases.
- **Five icons are stand-ins.** The OpenAEC icon set has no icons yet for solid, wireframe, edges, perspective and orthographic; the viewer uses the closest ones.
- **No walk mode, hidden-line mode or several models at once.**

Found something? [Open an issue](https://github.com/OpenAEC-Foundation/open-3d-viewer/issues).

## Under the hood

The viewer is written in Rust. The interface is [iced](https://iced.rs) 0.14 and the 3D view is an own wgpu pipeline with 4x MSAA inside iced's `shader` widget, so the same code runs natively and as WebAssembly on WebGPU, without a web view or JavaScript framework. The window follows the OpenAEC standard application: the shell, design tokens, fonts and B2 icons come from [OpenAEC Blueprint](https://github.com/OpenAEC-Foundation/OpenAEC-Blueprint) 2026.44.1 (`vendor/openaec-ui`, `assets/b2`). On a computer with integrated AMD graphics, a Revit sample of 18.8 MB loads in about 2.5 s and draws at about 110 frames per second at 1920x1080 (`open-3d-viewer --bench`).

## Contributing

- **Try it on a real project** and report bugs or missing features as an [issue](https://github.com/OpenAEC-Foundation/open-3d-viewer/issues/new). Describe what you did, what you expected and what happened, and attach the file if you can share it.
- **Share test files.** Models that open wrongly help most; `test-models/` holds the ones the tests use.
- **Write code** and open a pull request. `cargo test --release` must pass.

## Quick start (developers)

```bash
git clone https://github.com/OpenAEC-Foundation/open-3d-viewer.git
cd open-3d-viewer
cargo run --release -- test-models/test-kavel.skp
```

Requires Rust 1.88 or later (with the MSVC toolchain on Windows). The browser version also needs `rustup target add wasm32-unknown-unknown` and `cargo install --locked trunk`; then `trunk serve --release` serves it on `http://127.0.0.1:8080`. The Revit build is in `revit/` and expects the rvt-reader checkout next to this repository. The scripts in `scripts/` need Python 3.

The command line:

```
open-3d-viewer [FILE] [--theme light|dark|blueprint|high-contrast-dark|high-contrast-light] [--language en|nl]
open-3d-viewer [FILE] --screenshot OUT.png [--size 1440x900] [--mode solid|wireframe|edges] [--view iso|top|front|right]
               [--ortho] [--page workspace|file|settings|shortcuts] [--schedule NAME] [--explode X,Y,Z]
open-3d-viewer FILE --save-as OUT.ifc
open-3d-viewer --bench FILE [--frames 300]
```

Versions follow the OpenAEC scheme `YYYY.W.N`: year, ISO week without a leading zero, and a number within the week that starts at 0, such as `2026.44.0`. A tag `vYYYY.W.N` starts [`release.yml`](.github/workflows/release.yml).

Open 3D Viewer Studio is part of the [OpenAEC Foundation](https://github.com/OpenAEC-Foundation) family of apps.

## License

LGPL-3.0-or-later. See [LICENSE](LICENSE). The Inter font is under the SIL Open Font License (`assets/fonts/Inter-LICENSE.txt`); `vendor/openaec-ui` and its fonts and icons are described in its [`NOTICE.md`](vendor/openaec-ui/NOTICE.md). The private `rvt-reader` is linked only into the Revit build, never into the releases or the browser version.

## Trademarks

SketchUp is a trademark of Trimble Inc., Revit of Autodesk, Inc., and Tekla Structures of Trimble Solutions Corporation. Open 3D Viewer Studio is an independent project and is not affiliated with, endorsed by or sponsored by them.
