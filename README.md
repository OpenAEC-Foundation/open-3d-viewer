**English** · [Nederlands](README.nl.md)

<div align="center">

# Open 3D Viewer Studio

**A free 3D viewer for building models: it opens IFC, Revit, SketchUp, Tekla, Allplan, Archicad, DWG/DXF, MicroStation DGN, CityJSON, CityGML, GeoJSON, Shapefile, LandXML, glTF, USD, COLLADA, 3MF, FBX, 3DS, VRML/X3D, OBJ, OFF, STL and PLY models and LAS, LAZ, E57, PTS, PTX, PCD and ASCII XYZ point clouds, measures and sections them, and saves them as IFC, on the desktop and in the browser.**

[![Release](https://img.shields.io/github/v/release/OpenAEC-Foundation/open-3d-viewer?include_prereleases&label=release&color=D97706)](https://github.com/OpenAEC-Foundation/open-3d-viewer/releases/latest)
[![Build](https://github.com/OpenAEC-Foundation/open-3d-viewer/actions/workflows/build.yml/badge.svg)](https://github.com/OpenAEC-Foundation/open-3d-viewer/actions/workflows/build.yml)
[![Release workflow](https://github.com/OpenAEC-Foundation/open-3d-viewer/actions/workflows/release.yml/badge.svg)](https://github.com/OpenAEC-Foundation/open-3d-viewer/actions/workflows/release.yml)
[![License](https://img.shields.io/badge/license-LGPL--3.0-blue)](#license)

### [Try in your browser](https://open-3d-viewer.open-aec.com) &nbsp;·&nbsp; [Download](https://github.com/OpenAEC-Foundation/open-3d-viewer/releases/latest) &nbsp;·&nbsp; [Watch the video](https://www.youtube.com/watch?v=cxHO8M8W2Ow)

[![The product video of Open 3D Viewer Studio on YouTube: a model is opened, turned, cut open, pulled apart, measured and saved as IFC](https://img.youtube.com/vi/cxHO8M8W2Ow/maxresdefault.jpg)](https://www.youtube.com/watch?v=cxHO8M8W2Ow)

*▶ The product video (73 s) on the [OpenAEC Foundation channel](https://www.youtube.com/@OpenAECFoundation).*

</div>

## What this repository is

This repository is the complete source of **Open 3D Viewer Studio**, the 3D viewer of the [OpenAEC Foundation](https://github.com/OpenAEC-Foundation). Architects, engineers and builders send each other models in many formats; with this viewer you open any of them without the program that made it. You walk around the model, pick an element and read its properties, measure, cut it open, pull it apart per storey or per element, and save it as IFC.

One Rust code base gives three things: the **desktop app** for Windows, macOS and Linux, the **browser version** at [open-3d-viewer.open-aec.com](https://open-3d-viewer.open-aec.com), and the **MCP server** through which AI agents drive the desktop app. The repository also holds what turns that code into signed installers and a live website.

| Folder | What is in it |
|--------|---------------|
| [`src/`](src/) | The viewer: the window ([`shell.rs`](src/shell.rs)), the app state ([`app.rs`](src/app.rs)), the 3D view ([`viewport/`](src/viewport/)), measuring, sections, the exploded view, and the remote control for the MCP server |
| [`src/loaders/`](src/loaders/) | One adapter per file format; [`src/export/`](src/export/) writes IFC 4 |
| [`src/bin/open-3d-viewer-mcp.rs`](src/bin/open-3d-viewer-mcp.rs) | The MCP server for AI agents |
| [`vendor/openaec-ui/`](vendor/openaec-ui/), [`assets/b2/`](assets/b2/) | The shared OpenAEC window shell and icons, copied from [OpenAEC Blueprint](https://github.com/OpenAEC-Foundation/OpenAEC-Blueprint) 2026.44.1 |
| [`packaging/`](packaging/) | The Windows installer, the macOS app bundle and the Linux packages |
| [`.github/workflows/`](.github/workflows/) | `build.yml` tests every change, `release.yml` builds and signs a release, `live.yml` publishes the browser version |
| [`scripts/`](scripts/) | The README images, the B2 icon copy and the product video, all made by script |
| [`test-models/`](test-models/) | The models the tests open |

The readers for SketchUp, Tekla and PLY, and the Revit library, live in their own OpenAEC repositories ([`skp-reader`](https://github.com/OpenAEC-Foundation/skp-reader), [`tekla-reader`](https://github.com/OpenAEC-Foundation/tekla-reader), [`ply-reader`](https://github.com/OpenAEC-Foundation/ply-reader), [`rvt-dll`](https://github.com/OpenAEC-Foundation/rvt-dll)). This repository pins each to a fixed commit.

![A SketchUp model of a house on its plot in the viewer, with the models and layers on the left and the file properties on the right, theme Light](docs/images/readme/hero.png)

## Try it

**[Open the browser version](https://open-3d-viewer.open-aec.com)**. Drop a model on the page or choose **Open**, then orbit with the left mouse button, pan with the middle button and zoom with the wheel. You need a browser with WebGPU: a current Chrome, Edge, Firefox or Safari. Your file stays on your computer; nothing is uploaded.

## Download

Every release is on the [releases page](https://github.com/OpenAEC-Foundation/open-3d-viewer/releases/latest). The easiest way is the [OpenAEC Installer](https://github.com/OpenAEC-Foundation/openaec-installer): it installs, updates and starts every OpenAEC tool.

| Platform | Files |
|----------|-------|
| **Windows** | `.exe` installer (x64, no administrator rights needed) or `.zip` |
| **macOS** | `.dmg` (universal: Apple silicon and Intel) |
| **Linux** | `.deb`, `.rpm`, `.AppImage` or `.tar.gz` |

When the OpenAEC Installer has a newer version, the title bar of the viewer says so, and **Update** hands over to the installer. Every file is signed: Windows with Authenticode, macOS with a Developer ID and notarized by Apple, Linux with the OpenAEC OpenPGP key (`.asc` next to each package and to `SHA256SUMS`), and all of them with a GitHub build attestation. The release notes say per platform whether it is signed; a release that misses a signature is marked as a pre-release.

To check a download yourself:

```bash
sha256sum -c SHA256SUMS --ignore-missing
gpg --import docs/openaec-releases.asc   # OpenAEC Foundation Releases, fingerprint 13F6 D414 ADEE 4ED4 CD09 AC60 601D 86C9 3C50 4D29
gpg --verify SHA256SUMS.asc SHA256SUMS
gh attestation verify open-3d-viewer_2026.45.10_amd64.deb --repo OpenAEC-Foundation/open-3d-viewer
```

## OpenAEC Linux (SuperCloud)

The Linux release also runs in **OpenAEC Linux**, the environment of the OpenAEC SuperCloud in which each user's tools run on the server as regular Linux programs, streamed to the browser ([OpenAEC-Blueprint#61](https://github.com/OpenAEC-Foundation/OpenAEC-Blueprint/issues/61)). The SuperCloud builds that one image from the AppImage of a release, so this repository publishes no Docker image of its own.

- **Without FUSE:** `./open-3d-viewer_<version>_amd64.AppImage --appimage-extract`, then `squashfs-root/AppRun [FILE]`.
- **Without a GPU:** the 3D view draws with Mesa's software renderers, lavapipe (Vulkan, `mesa-vulkan-drivers`) or llvmpipe (OpenGL, `libegl-mesa0`). Without either, the window opens but the 3D view stays empty.
- **Under Wayland**, with a small compositor such as labwc, and without a system tray, notification service, keyring or D-Bus session.
- **Stopping:** the viewer only reads models and writes exports, so there is no unsaved work; on `SIGTERM` it stops.

| Variable | What it does |
|----------|--------------|
| `OPENAEC_BEHEERD=1` | Managed mode: no update notice (the SuperCloud refreshes the image). The viewer never downloads updates itself. |
| `OPENAEC_MAP` | The work folder (`/openaec` in the SuperCloud): the open and save dialogs start there. |
| `XDG_CONFIG_HOME` | Settings (preferences, own shortcuts, the MCP link) in `$XDG_CONFIG_HOME/OpenAEC/Open 3D Viewer Studio`, by default under `~/.config`. |
| `O3V_REMOTE=0` | No control port for the MCP server. `O3V_REMOTE_PORT` fixes its port (otherwise a free one). |
| `WGPU_BACKEND` | `vulkan` or `gl` to choose the graphics backend; by default the first that works. |

A file to open goes on the command line, as do `--theme light|dark|blueprint|high-contrast-dark|high-contrast-light` and `--language en|nl`.

**Network:** the viewer makes no outgoing connections and has no telemetry. It listens only on `127.0.0.1`, for its own MCP server (see [MCP](#mcp)). The file dialogs go through the XDG desktop portal, or `zenity` when there is none.

Every pull request and every release starts the Linux build this way on GitHub Actions ([`packaging/linux/headless-test.sh`](packaging/linux/headless-test.sh)): the release extracts the AppImage, starts it under labwc without a screen or GPU, checks that the window and a model in the 3D view draw, keeps the screenshots, and puts the memory in use with no file open in the release notes.

## Why use it?

- **Free and open.** LGPL-3.0. No licence fees, no account, no upload.
- **Opens what you get sent.** IFC (also zipped) and Revit on the desktop; SketchUp (version 3 and later), Tekla Structures, Allplan, Archicad (walls with their windows and doors, slabs, columns, beams, roofs, library objects as boxes, terrain meshes, zones, morphs, stairs and railings), DWG/DXF, MicroStation DGN (V7 and V8), CityJSON, CityGML, GeoJSON, Shapefile, LandXML, glTF, USD, COLLADA, 3MF, FBX, 3DS, VRML/X3D, OBJ, OFF, STL and PLY, and LAS, LAZ, E57, PTS, PTX, PCD and ASCII XYZ point clouds, on the desktop and in the browser.
- **Several models at once.** **Open** replaces the model, **Add** puts another file next to it, and each model has its own visibility toggle.
- **Saves as IFC.** Any model the viewer opens can be saved as IFC 4, with storeys, classes, properties and materials.
- **The same app in the browser and on the desktop.** One code base; the browser version needs no installation.
- **Measure and cut.** Distances, angles, areas and polygons, a section box and a section plane with filled cuts, and an exploded view.
- **AI agents can drive it.** An MCP server lets Claude and other agents open files, look around, select and measure, in the window you see.

## Screenshots

| | |
|---|---|
| ![The house model in the four themes Light, Dark, Blueprint and High contrast dark](docs/images/readme/themes.png) **Light, Dark, Blueprint and High contrast dark** | ![The plot from above in wireframe with an orthographic camera, theme Dark](docs/images/readme/plan.png) **Top view, wireframe and orthographic** |
| ![A bathroom model pulled apart per element](docs/images/readme/explode.png) **Exploded view per element** | ![The File page with Open, Add, Save as, Settings and the recent files](docs/images/readme/file.png) **The File page with recent files** |
| ![The settings dialog with theme, workspace, status bar, interface size and language, theme Blueprint](docs/images/readme/settings.png) **Settings, theme Blueprint** | ![The shortcuts page in the settings, where every command can get its own key](docs/images/readme/shortcuts.png) **Your own shortcuts for every command** |

## Viewing and navigating

- **Orbit, pan and zoom.** Left drag orbits, middle drag pans, the wheel zooms to the cursor. The view cube turns the model to a side; **F** fits the model in view.
- **Standard views.** Iso, top, front and right (`0`, `1`, `2`, `3`), perspective or orthographic (`P`, `O`), and the saved views of a Revit model.
- **Render modes.** Solid, wireframe and solid with edges (`S`, `W`, `E`), the ground grid (`G`) and **Flip up** for files with the other up axis.
- **Models, layers and properties.** The explorer lists the open models and their layers (IFC presentation layers or classes, Revit categories, SketchUp tags, Tekla classes, Allplan element categories, Archicad walls, slabs, columns, beams, roofs, openings, objects, terrain, zones, morphs, stairs and railings, CAD layers) with their colour and count; a click on an element shows its properties. IFC spaces and opening volumes start hidden in layers of their own; the openings stay cut from their walls and slabs.
- **Shortcuts.** Every command has one entry in one list, used by the ribbon, the title bar and the keyboard. In **Settings** you give any command your own key.

## Measure, section and explode

- **Measure** distances, angles, polygons and areas by clicking in the model, or between two elements. Before you click, the point, edge or face under the cursor is highlighted, and the cursor snaps to corners, edge middles and edges within a few pixels. Measurements stay in the view until you clear them.
- **Section box and section plane** cut the model open, with the cut faces filled. The box can fit the selection.
- **Exploded view** pulls the model apart per storey or per element, by a percentage per axis.

## Revit

The desktop app opens Revit files (`.rvt`, `.rfa`) through the OpenAEC Revit library [`rvt-dll`](https://github.com/OpenAEC-Foundation/rvt-dll). It shows the levels, grids, rooms, saved views and schedules of the file. A schedule opens in its own tab as a table under the 3D view, and a row selects its elements. Revit keeps only the definition of a schedule in the file, so the viewer computes the rows from the elements; cells it cannot work out yet show `?`.

![A Revit model with its Door Schedule opened as a table under the 3D view, theme Dark](docs/images/readme/schedule.png)

The images on this page are made by [`scripts/build_readme_images.py`](scripts/build_readme_images.py), which renders the real window with `open-3d-viewer --screenshot`. The schedule image uses the Autodesk sample `snowdon_arch_2025.rvt`.

## Interoperability

| Format | Open | Save |
|--------|------|------|
| **IFC 4** (`.ifc`, zipped `.ifczip`) | Desktop | Yes |
| **Revit** (`.rvt`, `.rfa`) | Desktop | No |
| **SketchUp** (`.skp`, version 3 and later) | Yes | No |
| **Tekla Structures** (`.db1`, beta) | Yes | No |
| **Allplan** (`.ndw` drawing files, `.prj` project folders, beta) | Yes, 3D bodies, macros and 2D lines; project folders on the desktop | No |
| **Archicad** (`.pln`, `.pla`, Archicad 24 to 29, beta) | Yes, walls with openings, slabs, columns, beams, roofs, library objects as boxes, terrain meshes, zones, morphs, stairs, railings | No |
| **DWG / DXF** (`.dwg`, `.dxf`) | Yes, polygon geometry | No |
| **MicroStation DGN** (`.dgn`, V7 and V8) | Yes | No |
| **CityJSON** (`.json`, `.cityjson`, `.jsonl`) | Yes | No |
| **CityGML** (`.gml`, `.citygml`, 1.0 to 3.0) | Yes | No |
| **GeoJSON** (`.geojson`, polygons and lines) | Yes | No |
| **Shapefile** (`.shp`, with the `.dbf` attributes on the desktop) | Yes | No |
| **LandXML** (`.xml`, `.landxml`, TIN surfaces) | Yes | No |
| **glTF** (`.gltf`, `.glb`) | Yes | No |
| **USD** (`.usd`, `.usda`, `.usdc`, `.usdz`) | Yes | No |
| **COLLADA** (`.dae`) | Yes | No |
| **VRML / X3D** (`.wrl`, `.x3d`, `.x3dv`, also gzipped) | Yes | No |
| **Wavefront OBJ** (`.obj`) | Yes | No |
| **STL** (`.stl`, ASCII and binary) | Yes | No |
| **PLY** (`.ply`, meshes and point clouds) | Yes | No |
| **3MF** (`.3mf`) | Yes | No |
| **FBX** (`.fbx`, binary and ASCII) | Yes | No |
| **3D Studio** (`.3ds`) | Yes | No |
| **OFF** (`.off`, ASCII) | Yes | No |
| **LAS / LAZ** point clouds (`.las`, `.laz`) | Yes | No |
| **E57** point clouds (`.e57`) | Yes | No |
| **ASCII XYZ** point clouds (`.xyz`, `.txt`, `.csv`, `.asc`) | Yes | No |
| **Leica PTS / PTX** point clouds (`.pts`, `.ptx`) | Yes | No |
| **PCD** point clouds (`.pcd`, Point Cloud Library) | Yes | No |

What does not come through:

- **IFC export** writes each element as triangles (`IfcTriangulatedFaceSet`), not as parametric geometry. Classes come from the Revit category, the Tekla part name or the Archicad element (wall, slab, column, beam, roof plane as a roof slab), else `IfcBuildingElementProxy`; Archicad openings are already cut from their walls and are not written, Archicad library objects are `IfcBuildingElementProxy`, meshes `IfcGeographicElement`, zones `IfcSpace`, morphs `IfcBuildingElementProxy`, stairs `IfcStair` and railings `IfcRailing`; elements of an opened IFC file do not keep their class yet. Checked with IfcOpenShell 0.8 on Revit, SketchUp, OBJ, STL and glTF models.
- **DWG and DXF** show the polygon geometry of model space. ACIS solids, annotations, curves and external references are left out and counted.
- **MicroStation DGN** opens version 7 design files and V8 design files (MicroStation V8, V8i and CONNECT). Lines, arcs and curves are drawn as thin ribbons as wide as their line weight, text as its box with the characters in the element properties; shapes, cones, extruded solids and B-spline surfaces of 3D files are faces. Levels are layers, with their names in V8 files; cells and shared cells are one element each; 2D linear dimensions of V8 files are drawn as lines, with the measured values in the element properties. Other dimensions, multi-lines, tags, line styles and reference files, and in V8 files surfaces, solids, cones, B-spline surfaces, meshes and models other than the first, are left out and counted. The V7 reader is written from Intergraph's public ISFF description and a clean-room analysis, the V8 reader from the clean-room specification only.
- **LandXML** shows TIN surfaces; survey points and alignments are not turned into surfaces. **CityJSON** shows the highest level of detail per object.
- **CityGML** shows the polygons of each city object at its highest level of detail (LoD1 blocks or LoD2 surfaces), one layer per surface type (walls, roofs, ground, windows, doors, ..) or else per object type, one element per city object with its attributes and generic attributes; coordinates are moved to a local origin, not reprojected. Textures and colours (appearances), implicit geometry (placed library objects), geometry referenced from other objects and curves are not read.
- **GeoJSON** shows polygons as flat faces (with their holes) and lines as 0.3 m wide ribbons, one element per feature with its properties; a polygon with a `height`, `building:height` or `building:levels` (3 m a level, with `min_height` for the underside) becomes a block. Longitude/latitude are projected to local metres around the first coordinate (equirectangular, fine for a district, not for a country); projected coordinates are taken as metres. Points are not drawn. A polygon that is not flat (a wall) is triangulated in its own plane.
- **Shapefile** shows polygons (2D, Z and M) as faces with their holes, extruded into a block when a record has a height attribute (as for GeoJSON, in any case), lines as 0.3 m wide ribbons, MultiPatch strips, fans and rings as faces in a `Buildings` layer, and points as dots. Each record is a pickable element with the attributes of the `.dbf` next to the file, in its code page (`.cpg` or the `.dbf` header); the `.prj` names the coordinate system. Longitude/latitude are projected to local metres; projected coordinates are moved to a local origin, not reprojected. In the browser only the `.shp` is read, so there are no attributes. M values, `.shx` indexes and zipped shapefiles are not read. The reader is the pure-Rust `shapefile` crate.
- **Textures** of glTF, OBJ and SketchUp become their average colour; SketchUp soft edges are not smoothed.
- **COLLADA** shows the triangles and polygons of the visual scene, one layer per SketchUp tag or else per material, in the diffuse colour of the material; textures, lines, polygons with holes and animations are left out. **3MF** shows each build item as a layer, with its components, base materials and colour groups; textures, beam lattices and slices are not read.
- **VRML / X3D** (VRML 2.0 / VRML97, X3D in XML and classic encoding, also gzipped) flattens the scene graph (`Transform`, `Group`, `Switch`, `LOD`, `DEF`/`USE`) and shows `IndexedFaceSet` and the other triangle and quad sets, `ElevationGrid`, and `Box`, `Sphere`, `Cylinder` and `Cone` tessellated, one layer per material in its diffuse colour, each `Shape` a pickable element named after its `DEF`; `PointSet` as dots. VRML 1.0, textures, `Extrusion`, lines, text, `Inline` files, `PROTO` instances, scripts and animations are not read. The reader is written for this viewer from the ISO specifications.
- **USD** shows the meshes of the composed stage (sublayers, references, payloads, variants and instances), each mesh a pickable element with its name and path, placed with its transform ops, one layer per bound material in its `UsdPreviewSurface` diffuse colour, or else in its `displayColor`; up axis and units come from the stage. Textures, subdivision surfaces, implicit shapes (cube, sphere, ..), curves, points, skinning and animation (the default time is drawn) are left out. In the browser only the file itself is read, so references to other files are missing (a `.usdz` carries its own). The reader is the pure-Rust `openusd` crate.
- **FBX** shows the meshes of the scene, each model a pickable element with its name and user properties, placed with its parents and its full transform, one layer per material in its diffuse colour; units and axes come from the file. Textures, NURBS and curves, cameras, lights, animations, skinning and blend shapes are left out. The reader is written from the public description of the format, without the FBX SDK. **3DS** shows each mesh object as an element, one layer per material in its diffuse colour, in file units; textures, smoothing groups, lights, cameras and the keyframer (animation, instances) are not read. **OFF** shows one flat shaded mesh with its vertex or face colours; binary OFF is not read.
- **Point clouds** are drawn as dots of a few pixels. **LAS/LAZ** (1.0 to 1.4, LASzip compressed) shows a layer per classification class (Ground, Building, High vegetation, ...), each point in its RGB colour, or else in the colour of its class shaded by its intensity. **E57** shows each scan as a layer, placed by its pose, in its colours or else its intensity as grey; images in the file are not read. **PLY** without faces is a point cloud, in its colours, else its intensity, with a layer per class when it has a classification. **ASCII XYZ** (`.xyz`, `.txt`, `.csv`, `.asc`) reads X Y Z per line with optional colour (0..255, 0..1 or 16 bits), intensity and classification columns, named by a header line (also CloudCompare's `//X,Y,Z`) or else found from their values; separated by spaces, tabs, commas or semicolons (then with decimal commas). A `.txt` or `.csv` opens only when its lines are columns of numbers; other columns (normals, scalar fields) are not read. **PTS** shows each scan (a count line) as a layer, X Y Z, intensity, R G B. **PTX** shows each scan as a layer, moved by its registration matrix; points without a return are left out. **PCD** reads ASCII, binary and `binary_compressed` data with packed `rgb`/`rgba` or separate colours, else intensity; its `VIEWPOINT` is shown, not applied, and other fields (normals, labels) are not read. Points without colour or intensity are coloured by height. A cloud of more than 10 million points (4 million in the browser) shows every n-th point; the properties tell how many are in the file and how many are shown. Survey coordinates are moved to a local origin, which the properties show (`LAS world origin`, `XYZ world origin`, ..). Points cannot be selected, have no cut fill, and stay in place in the exploded view; saving as IFC leaves them out.
- **Tekla** profiles are approximations; the reader is written from a clean-room specification.
- **Allplan** opens a drawing file (`.ndw`) and shows its 3D bodies (walls, slabs, columns, windows, stairs) in a layer per category; openings and rooms start hidden in a layer of their own. A click on a body shows the name, element type and quantities (height, thickness, length, area, volume) of its Allplan element. Macros (furniture, windows, doors, railings) are placed in a `Macros` layer, one element per placement. Opened from its project folder, each body gets its colour from the project: its surface (`design/*.surf`, a textured surface by the average of its preview) or else its colour number in `animation.cfg`; in the browser, or for a drawing file on its own, the layers keep a fixed colour per category. On the desktop a whole project folder (`*.prj`, dropped on the window) opens as one model: the drawing files of its building structure, one layer per storey (the category becomes a property), and only the first building of the structure shown. 2D lines and polylines that are not part of a 3D element (site plans, axes, plan details) are drawn as thin dark lines just above their storey, in a `2D lines` layer. Textures, texts, hatchings, arcs and layouts (`.npl`) are not read yet. The reader is written from a clean-room specification.
- **Archicad** shows walls, slabs and columns as extruded footprints on their stories, beams along their axis, sloped ones too (columns and beams with an own profile as that profile, its arcs included), and each plane of a roof between its sloped underside and top, with vertical edges. Arcs in outlines are drawn as short straight segments; roofs of thickness 0 (helper bodies for solid element operations) are left out. Windows and doors are cut from their walls as openings, which start hidden in a layer of their own. Library objects (furniture, kitchens, sanitary, 3D text) are drawn as boxes of their size, 1 m high when the height is in the library part's own code. Meshes become closed terrain bodies (their triangulated top, a flat bottom and sides), and zones prisms that start hidden, half transparent, in a layer of their own. Morphs are drawn face by face from their stored body, without materials. Stairs are a block per tread from the stair's plan scheme, railings a half transparent band with a rail on top and posts at the segment ends; the real shapes of treads, stringers, posts and balusters (library code) are not read. Window and door frames, glass and leaves, the real shapes of library objects, curved roofs and shells, zones cut by roofs (drawn whole), and projects from before Archicad 24 are not read yet. Wall heights follow the story a wall's top is linked to, or the wall's own height. The reader is written from a clean-room specification.
- **In the browser**, IFC and Revit do not open, `.gltf` files need their buffers embedded, and OBJ materials are not read.

New formats: implement `loaders::Loader` and add it to `loaders::registry()`; new writers implement `export::Writer` and go in `export::registry()`.

## MCP

`open-3d-viewer-mcp` is an [MCP](https://modelcontextprotocol.io) server through which an AI agent (Claude Code, Claude Desktop or any MCP client) drives the desktop viewer as a person does: open files, take screenshots, set the camera and the standard and saved views, show, hide and isolate layers, find elements by property, select, read properties, set a section box or plane, pull the model apart, change the render mode, read Revit schedules and measure.

- **You see what it does.** Commands go through the same path as your clicks, in the window on your screen. Measurements stay in the view.
- **It stays on your computer.** The window listens on `127.0.0.1` only, with a random token in `%APPDATA%\OpenAEC\Open 3D Viewer Studio\remote.json`. `--no-remote` or `O3V_REMOTE=0` switches it off.
- **Install:** keep `open-3d-viewer-mcp` next to the viewer (the installers do) and register it, for example `claude mcp add open-3d-viewer -- "C:\path\to\open-3d-viewer-mcp.exe"`. `open-3d-viewer-mcp --print-config` prints the JSON for other clients.

## Graphics

The window draws with wgpu. On Windows it uses DirectX 12 when the graphics card is an Intel one, because Intel's Vulkan driver can flash the whole window white while you orbit ([#28](https://github.com/OpenAEC-Foundation/open-3d-viewer/issues/28)), and OpenGL on Intel chips too old for wgpu's DirectX 12 (Haswell, Broadwell); other cards keep wgpu's choice. Each start writes what the window got (card, driver, backend, surface format) to `gpu.log` in the settings folder: `%APPDATA%\OpenAEC\Open 3D Viewer Studio` on Windows, `~/.config/OpenAEC/Open 3D Viewer Studio` on Linux and macOS.

If the window flickers, stays black or is slow, try another backend or present mode, and mention which one helps in an issue together with `gpu.log`:

- **Once:** `open-3d-viewer.exe --gpu-backend vulkan` (or `dx12`, `gl`) and `--present-mode mailbox` (or `fifo`, `vsync`, `no_vsync`). Mailbox works with DirectX 12 and Vulkan, not with OpenGL.
- **Always:** set the user variable `WGPU_BACKEND` or `ICED_PRESENT_MODE` to one of these values, for example `setx WGPU_BACKEND vulkan` in a command prompt, and start the viewer again.

## Testing and reliability

- **209 unit tests** in `src/` (207 run; 2 are left out by default, one needs a local sample), plus 2 for the MCP server and 56 for the window library in `vendor/openaec-ui`, run on every pull request and before every release: 119 for the loaders, 16 for the app, 17 for measuring, 8 for the window shell, 7 for the section tools, 10 for IFC export, and more for the 3D view, view cube, remote control, model and exploded view.
- **The window shell is tested without a screen.** One test lays out every view and fails when a control has no name for a screen reader; others check that no two commands share a shortcut and that every command in the ribbon and the title bar exists.
- **Generated files are checked.** `python scripts/build_readme_images.py --check` fails when an image is missing, unused or larger than 150 kB; `python scripts/b2_icons.py --check` fails when an icon differs from the OpenAEC Blueprint release named in `assets/b2/blueprint.sha256`, without a copy of the Blueprint.
- **IFC export** is checked with IfcOpenShell 0.8 (schema and where rules, spatial containment, unique GlobalIds, geometry) on Revit samples of 456 and 6773 elements, 11 SketchUp test models, OBJ, STL and glTF.
- **The Linux build starts without a GPU.** On every pull request and release it runs under labwc without a screen, with software rendering, and must draw a model (see [OpenAEC Linux](#openaec-linux-supercloud)).
- **Releases are built only after the tests pass**, and the release notes state per platform whether the files are signed.

Count sources: `cargo test --release` and `cargo test --release -p openaec-ui --lib`; the release runs the same checks in [`release.yml`](.github/workflows/release.yml).

## Current limitations

- **IFC and Revit open on the desktop only.** The browser version does not include those readers.
- **Some formats are missing.** STEP and IGES do not open.
- **No undo yet.** Measurements and the section are cleared by hand.
- **The browser version blocks while it loads** a large model (no worker threads yet) and does not keep your settings between visits.
- **Five icons are stand-ins.** The OpenAEC icon set has no icons yet for solid, wireframe, edges, perspective and orthographic; the viewer uses the closest ones.
- **No walk mode or hidden-line mode.**

Found something? [Open an issue](https://github.com/OpenAEC-Foundation/open-3d-viewer/issues).

## Under the hood

The viewer is written in Rust. The interface is [iced](https://iced.rs) 0.14 and the 3D view is an own wgpu pipeline with 4x MSAA inside iced's `shader` widget, so the same code runs natively and as WebAssembly on WebGPU, without a web view or JavaScript framework. The window follows the OpenAEC standard application: the shell, design tokens, fonts and B2 icons come from [OpenAEC Blueprint](https://github.com/OpenAEC-Foundation/OpenAEC-Blueprint) 2026.44.1 (`vendor/openaec-ui`, `assets/b2`). IFC is read with [IFClite](https://github.com/LTplus-AG/ifc-lite) and DWG/DXF with [acadrust](https://github.com/HakanSeven12/acadrust). On a computer with integrated AMD graphics, a Revit sample of 18.8 MB loads in about 2.5 s and draws at about 110 frames per second at 1920x1080 (`open-3d-viewer --bench`).

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

Requires Rust 1.89 or later (with the MSVC toolchain on Windows) and read access to the private OpenAEC format repositories above; set `CARGO_NET_GIT_FETCH_WITH_CLI=true` so Cargo uses your Git credentials. The browser version also needs `rustup target add wasm32-unknown-unknown` and `cargo install --locked trunk`; then `trunk serve --release` serves it on `http://127.0.0.1:8080`. The scripts in `scripts/` need Python 3.

The command line:

```
open-3d-viewer [FILE] [--theme light|dark|blueprint|high-contrast-dark|high-contrast-light] [--language en|nl]
open-3d-viewer [FILE] --screenshot OUT.png [--size 1440x900] [--mode solid|wireframe|edges] [--view iso|top|front|right]
               [--ortho] [--page workspace|file|settings|shortcuts] [--schedule NAME] [--explode X,Y,Z]
open-3d-viewer FILE --save-as OUT.ifc
open-3d-viewer --bench FILE [--frames 300]
```

Versions follow the OpenAEC scheme `YYYY.W.N`: year, ISO week without a leading zero, and a number within the week that starts at 0, such as `2026.45.10`. A tag `vYYYY.W.N` starts [`release.yml`](.github/workflows/release.yml), which tests, builds and signs the release for Windows, macOS and Linux and publishes it.

Open 3D Viewer Studio is part of the [OpenAEC Foundation](https://github.com/OpenAEC-Foundation) family of apps.

## License

The viewer's own source is LGPL-3.0-or-later. See [LICENSE](LICENSE). The Inter font is under the SIL Open Font License (`assets/fonts/Inter-LICENSE.txt`); `vendor/openaec-ui` and its fonts and icons are described in its [`NOTICE.md`](vendor/openaec-ui/NOTICE.md). IFClite and acadrust are MPL-2.0. The desktop app links the OpenAEC Revit library `rvt-dll`, which keeps its own proprietary licence, so the desktop builds are not wholly open source; the browser version does not include it.

## Trademarks

SketchUp is a trademark of Trimble Inc., Revit and DWG of Autodesk, Inc., Tekla Structures of Trimble Solutions Corporation, Allplan of ALLPLAN GmbH (Nemetschek Group), MicroStation of Bentley Systems, Inc., and Archicad of Graphisoft SE. Open 3D Viewer Studio is an independent project and is not affiliated with, endorsed by or sponsored by them.
