# From viewer to BIM modeller: a phased design

**Status:** proposal, 2026-11-06. Nothing in this document is built yet.

## Intent

Open 3D Viewer Studio is a fast viewer: it opens some forty formats, turns every model into triangle meshes and draws them at a high frame rate on the desktop and in the browser. The aim is to grow it, step by step, into a BIM modeller with a real BIM kernel underneath: elements that you select, hide, stretch with handles, lengthen, bend into a curved wall, and in the end model from scratch, on models that may span a thousand kilometres.

The viewer's speed is the asset to protect. Every step must keep the camera at 120 frames per second. Rendering stays mesh-based; what changes is that the meshes become *derived* from editable element definitions, instead of being the only thing the app knows.

Open BIM Studio, the OpenAEC BIM modeller, has written down the requirements for such a kernel (performance, precision, tiles, representations, exchange, editing). Those requirements apply to this repository too, phased in as described below. Open BIM Studio is written in its own language (DynLex); this repository stays Rust. What carries over is the requirements, the decisions and the algorithms, not the code.

## Goals

- **Editing on top of viewing.** Select, hide and isolate elements, then manipulate them directly (handles, points), then change their parameters, then create new ones.
- **A kernel that grows by representation type.** Start with meshes and extruded profiles; add swept solids, B-rep and booleans when an editing step needs them.
- **120 fps throughout.** Camera-only frames never wait for kernel work, in any phase.
- **Large models.** f64 coordinates in tile frames, so a model can be built at survey coordinates and over kilometres without losing precision.
- **Honest exchange.** What the app edits, it writes to IFC as faithfully as it can, and it says what it loses.
- **Every phase ships.** Each phase ends in a release of the viewer that is better than the one before, with nothing half-built visible to users.

## Non-goals

- Structural, energy or other engineering analysis.
- Photoreal offline rendering.
- Full IFC coverage in one go. Elements the kernel does not understand stay read-only meshes.
- Real-time multi-user editing.
- A rewrite. The loaders, the renderer, the browser build and the MCP server stay and are extended.

## Anti-goals (failure even if it technically works)

- **A slower camera.** Any phase that drops camera-only navigation below the 120 fps criterion (see *Performance*) fails, also when the drop happens only while an edit is being evaluated.
- **Speed bought by leaving things out.** Dropping cut fills, layers or systems to make a frame budget counts as failure.
- **f32 in the modelling path.** Element definitions and kernel geometry are f64; f32 exists only in GPU buffers. An imported mesh element may keep its f32 vertices relative to its model's f64 origin as long as nobody edits it; the first edit converts that element to f64.
- **Silent conversions.** Mesh, B-rep, extrusion and point cloud are distinct representations; a conversion is always a named, explicit operation, and a refused operation stays a refusal.
- **Breaking the browser.** The wasm32 build (WebGPU, with the WebGL2 fallback) keeps working in every phase. Desktop-only features stay behind `cfg` gates as today.
- **A second geometry pipeline.** Section cuts, measurements and picking use the same kernel geometry as the display, not a parallel copy.
- **Lossy round trips that look lossless.** A file saved after an edit must not claim to be parametric where it is not.

## Where the viewer stands today

The facts the phases build on (file references are to `main` on 2026-11-06):

- **Model.** `Layer` (`src/model.rs`) is one triangle mesh in f32 with one base colour; elements are `(id, first index, index count)` ranges into its index buffer; properties are one JSON string per element. Nothing parametric survives loading: IFC (`src/loaders/ifc.rs`, via IFClite), STEP (B-rep decoded by `cadmpeg`, then dropped), IFCX, Archicad and Allplan all hand over triangles only.
- **Ids are not stable.** `Model::combine` renumbers element ids to `1..N` whenever a model is added.
- **Precision.** Each loader parses in f64 and subtracts its own anchor point; the anchor is kept only as a text value in the project information. There is no structured origin, and models with different anchors are not aligned.
- **Rendering** (`src/viewport/render.rs`). One immutable vertex and index buffer per layer (no `COPY_DST`); visibility per layer only; the selection is highlighted by redrawing its index ranges, one draw call per range. A per-element texture (`unit` per vertex, `Rgba32Float`, used for the exploded-view offsets) can be rewritten without touching geometry; its `w` channel is unused.
- **Picking** is on the CPU (ray against triangles, 256-triangle bounding boxes). Selection is one element at a time in the UI.
- **Handles.** The section box (`src/section.rs`) already has draggable handles: hit testing within 10 px, screen drag to world delta along an axis, hover and cursor states. Measuring (`src/measure/`) snaps to corners, edge midpoints, edges and faces.
- **No undo.** **No element hide.** **No editing.**
- **IFC export** (`src/export/ifc.rs`) writes IFC4 with one `IfcTriangulatedFaceSet` per element and colour; no extrusions, no `IfcMapConversion`.
- **Performance.** `--bench` renders offscreen and waits for the GPU after each frame, so it measures latency, not throughput. The README quotes about 110 fps at 1920x1080 for an 18.8 MB sample on integrated graphics.
- **A shared-core pattern exists.** The draft validator branch (`codex/rust-bim-validator`) adds a headless crate (`crates/bim-validator-core`) on the IFC entity graph (`ifc-lite-core`) next to the mesh model, and a second product built from the same code base by a Cargo feature.

## Requirements taken over from Open BIM Studio

Each requirement below comes from Open BIM Studio's decision records and risk register; the last column says from which phase it binds here.

| Requirement | Source in Open BIM Studio | Binds from |
|---|---|---|
| Camera-only navigation: at least 120 submitted fps, p95 frame time at most 8.33 ms, frames over 16.67 ms counted | `docs/research/interactive-view-performance.md` | Phase 0 (measured), every phase after (kept) |
| Camera-only motion never regenerates meshes; model edits and camera frames are separate workloads | same | Phase 2 |
| Selection scales to 10⁵ elements | `docs/domain/capability-inventory.md` | Phase 1 |
| Re-evaluating one edited element under 16 ms on a 100k-element model (over 100 ms fails) | `docs/risks.md`, R-01 | Phase 3 |
| Stable 64-bit element ids, never reused; immutable element values, so a shallow copy is an undo snapshot | `bim/bim_model.dl`, `docs/domain/parametric-components.md` | Phase 1 (ids), Phase 2 (undo) |
| Every change is one transaction and one undo step; previews never change the document; undo is instant and unlimited | `docs/ux/interaction-model.md`, `docs/superpowers/specs/2026-10-26-canvas-editing.md` | Phase 2 |
| Geometry is derived from parametric definitions, not stored as the source of truth | `docs/architecture/overview.md` | Phase 3 |
| Representations (mesh, B-rep, point cloud, implicit) are equal; no implicit conversion; refusals stay refusals | ADR-0005 | Phase 3 |
| Kernel coordinates are f64 in a local tile frame; f32 only on the GPU after a camera-relative step done in f64 | ADR-0006 | Phase 3 (one frame), Phase 5 (tiles) |
| Tiles are a coordinate frame and a loading unit, never a semantic boundary; elements belong to the tile of their placement origin and are never split | ADR-0016 (proposed) | Phase 5 |
| 10⁵ elements at a 500 km offset at 60 fps with coordinate error under 1e-3 m (here: at the 120 fps criterion) | `docs/risks.md`, R-20 | Phase 5 |
| Detail levels, proxies, tessellations and caches are derived view data; they never change the model or what is exported | ADR-0013 | Phase 3 |
| Display tessellation by chord tolerance (0.1 mm), never the modelling tolerance | `viewer/bim_display_detail.dl` | Phase 3 |
| Partial but correct exchange is usable, broad but lossy is not; supported IFC elements become editable, the rest stays read-only | `docs/vision/non-goals.md` | Phase 3 |
| IfcX (IFC5) as the reference for the native data model, IFC4 written by default | ADR-0010 | Phase 6 |
| Handles: eight first (four corners, four mid-sides), then the vertices; a first click selects, a second click or Tab goes one level deeper | `docs/open-questions.md`, R1-28 and R1-54 | Phase 2 |
| Dependencies MPL-2.0 or compatible, with a licence check per added dependency | ADR-0009, R-11 | every phase |

Two Open BIM Studio rules do **not** carry over: the browser is a requirement here (there it is a long-term goal), and product code here is Rust (there it is DynLex only).

## The architecture direction

### Two sides: the document and the display

- **The document** holds elements: a stable `ElementId(u64)`, a class, an optional type, a placement as an f64 frame, properties, and one or more representations. It lives in a new headless crate (working name `bim-core`, in a Cargo workspace like the validator's), with no GUI or GPU dependency, so it can be tested, used by the MCP server, and shared with other OpenAEC tools.
- **The display** is what exists today: f32 meshes in GPU buffers, per layer. It becomes a cache derived from the document. A loaded file that the kernel does not understand yet becomes elements whose only representation is a mesh, which is exactly today's behaviour.
- **Edits run off the render thread.** A worker evaluates the changed elements and publishes a new geometry revision atomically; the render loop picks it up between frames. A camera frame never waits for an evaluation; if the result is not ready, the frame draws the previous revision or the preview.

### Per-element state on the GPU

The per-element texture used by the exploded view becomes the general per-element channel: offset (xyz) plus flags in `w` (hidden, selected, hovered, edited). Hiding, isolating and highlighting then cost one texture write, not a re-upload and not one draw call per selected range. The texture is kept (not a storage buffer) because of the WebGL2 fallback.

### Changing geometry without re-uploading layers

An edited element gets the *edited* flag, which hides its original triangles in the static layer buffer, and its new triangles go into a small dynamic **edit layer** (with `COPY_DST`) that is rewritten on every revision. Big layers stay untouched while you drag. When the edit layer grows large, or on save, it is folded back into the static buffers in the background. Picking, measuring, the cut fill and the exploded view consult the same revision.

### The kernel grows by representation

1. **Mesh** (exists): triangles with an affine transform per element.
2. **Extruded profile** (Phase 3): a 2D profile with holes, a depth and a direction, in the element frame. Covers most walls, slabs, columns and beams in IFC (`IfcExtrudedAreaSolid`).
3. **Swept solid** (Phase 4): a profile along a directrix (line, arc, polyline), for curved walls and bent members (`IfcSweptDiskSolid`, `IfcFixedReferenceSweptAreaSolid`, `IfcSurfaceCurveSweptAreaSolid`).
4. **B-rep with booleans** (Phase 4): for openings, joins and STEP solids, which are then kept instead of dropped.
5. **Point cloud** stays a display-only representation until a phase needs to edit it.

Each type has an evaluator (definition to tessellated mesh, f64 in, f32 out per tile frame) and a set of handles it exposes. Adding a type is adding an evaluator and its handles; nothing else in the app changes.

## Phases

Each phase ends with exit criteria that are measured, and in a release.

### Phase 0: measure the frame budget

- Add a throughput mode to `--bench`: camera-only orbit, pan and zoom scenarios on fixed reference models, reporting submitted fps, p95 frame time and frames over 16.67 ms.
- Fix the reference models (a large IFC, a large `.rvt` sample, a point cloud) and the reference hardware, including one integrated GPU.
- Record the baseline in the repository.

**Exit:** the benchmark reports the 120 fps criterion per scenario, the baseline is written down for the reference PC and for integrated graphics, and every scenario meets the criterion on the reference PC (with renderer work in this phase if the baseline did not).

### Phase 1: selection and visibility

- **Multi-select:** Ctrl-click adds or removes, Shift-click adds, box selection, select all of a layer or a property value (the MCP `find_elements` already finds them).
- **Hide, isolate, show all** for elements, from the ribbon, the context of the selection, a shortcut and the MCP server.
- **Per-element GPU flags** for hidden, selected and hovered, replacing the per-range highlight pass.
- **Stable element ids** across Open and Add: a model-qualified id (`model index`, `source id`) instead of renumbering in `Model::combine`.
- **Undo and redo** for selection-independent view changes (hide, isolate, show), as the first users of the transaction log.

**Exit:** hiding or isolating 10⁵ elements takes effect in the next frame; selecting 10⁵ elements keeps the 120 fps criterion; ids stay the same after **Add**.

### Phase 2: direct manipulation on meshes

- **Handles** on a selected element: eight in its local frame (four corners, four mid-sides of its footprint), plus a height handle; Tab or a second click switches to its vertices.
- **Point selection:** select vertices and move them.
- **Drags** move or stretch with the measure snaps (corners, midpoints, edges), temporary ortho with Shift, and a typed distance during the drag.
- **Stretch moves, it does not scale.** Dragging a mid-side handle moves every vertex beyond a plane through the element's middle by the drag distance and leaves the rest where it is, so holes, profiles and roundings keep their shape. A corner handle does this along two axes at once.
- **The element frame** comes from the source placement where the loader has one (IFC, Archicad, Allplan, STEP), and otherwise from the element's oriented bounding box, with the model's up axis kept vertical.
- **Transactions:** every finished drag is one undo step; the preview during the drag is never written to the document.
- **The edit layer** carries the changed elements.
- **Saving** writes the edited meshes to IFC as triangulated geometry, as today, and says so in the save dialog's notes.

**Exit:** a stretch on a 100k-element model keeps the 120 fps criterion during the drag; undo restores the exact geometry; a saved IFC opens in an independent IFC toolkit with the edited geometry.

### Phase 3: the document and the first parametric element

- **`bim-core`** with the element model above, f64 placements, and one coordinate frame per model with a structured f64 origin (replacing the text value), read from and written to `IfcMapConversion`. Models with different origins are aligned on **Add**.
- **IFC import keeps the recipe** for elements whose body is an `IfcExtrudedAreaSolid` (on a rectangle, a polygon with holes, or a catalogue profile), using the IFC entity graph (`ifc-lite-core`, as in the validator branch). Everything else stays a mesh element.
- **Walls with openings stay editable.** Rectangular openings (`IfcRelVoidsElement` with an extruded opening) are cut by the direct construction ported from Open BIM Studio's `kernel/fast/`, without general booleans. Openings keep their distance from the wall's start, as in Open BIM Studio, so their windows and doors stay in place when the wall gets longer, and an opening that would fall off the wall refuses the edit. A wall whose body the direct construction does not accept (a boolean clipping it does not understand, a curved opening) stays a read-only mesh until Phase 4.
- **Parameter handles:** the end handle of a wall changes its length; the top handle its height; the side handle its thickness. The properties pane shows and edits the same parameters. Thickness is editable only when the wall has its own material layer set; when the layer set comes from a type shared with other walls, the thickness handle is not shown (editing types comes in Phase 6).
- **Evaluation** of the edited element runs on a worker and publishes a revision; the 16 ms budget per element applies.
- **Saving an opened IFC keeps the file.** The source entity graph stays in memory after loading (desktop), and saving writes it back in its own schema (IFC2X3, IFC4 or IFC4X3) with only the edited elements changed: their body representation, placement and the quantities that follow from them. Types, property sets, relations, GlobalIds and untouched geometry are written as they were read. This follows Open BIM Studio's round-trip rule (retain the source record graph, report losses). The save reports which elements changed and, if any, which edits could not be written parametrically.
- **IFC export of other formats** keeps working as today (`src/export/ifc.rs`); an element with an extruded representation is written as `IfcExtrudedAreaSolid`, a mesh element as `IfcTriangulatedFaceSet`. Classes and GlobalIds of an opened IFC are kept on this path too, which fixes the current loss.
- **Display tessellation** by chord tolerance (0.1 mm), with the segment count of round profiles bounded.

**Exit:** open an IFC with walls, drag a wall end, save: the saved file has a longer extrusion and the same GlobalId, and opens in an independent IFC toolkit; re-evaluation of the wall stays under 16 ms on a 100k-element model.

### Phase 4: swept solids and B-rep

- **Swept solids** along lines, arcs and polylines: a straight wall gets a handle that bends it into an arc, which makes it a swept solid.
- **A B-rep evaluator** for openings, joins and booleans. The first candidate is the Rust crate `cadkernel` (MPL-2.0), which Open BIM Studio ported and uses as its reference; its known gaps (no exact predicates, single-threaded, some refused boolean cases) are measured first. Where a wall with rectangular openings can be built directly without booleans, the direct construction from Open BIM Studio's `kernel/fast/` is ported instead.
- **STEP keeps its B-rep** as a representation instead of dropping it after tessellation.
- **Conversions are named commands** (B-rep to mesh, mesh to B-rep when it is closed), never implicit.

**Exit:** bend a wall with an opening into an arc and back at frame rate; the opening stays cut; undo restores both steps.

### Phase 5: tiles and kilometre-scale models

- **Tile frames:** a fixed square grid per project, each element in the tile of its placement origin; f64 kernel coordinates per tile; GPU buffers in f32 per tile frame, with the f64 difference to the camera's tile folded into the per-tile transform.
- **Phase 5a, GPU residency:** the document stays wholly in memory; tiles decide which GPU buffers are resident and at which derived detail level. This is enough for a 1000 km model that fits in memory.
- **Phase 5b, a paged document:** loading and unloading the document by tile, with a read-only halo of one tile; unsaved edits block unloading; dependencies into unloaded tiles are marked stale, and saving requires nothing stale. This needs a tile-addressable store, so it comes after the native file of Phase 6, or with a tile cache built for it.
- **Derived detail levels** for distant tiles, never written to the model or the export.

**Exit:** 10⁵ elements at a 500 km offset meet the 120 fps criterion with a coordinate error under 1e-3 m; a model of 1000 km extent navigates within the criterion.

### Phase 6 and later: modelling

Create walls, slabs, columns and beams with tools; types and occurrences; joins; a native file in IfcX; constraints. This phase gets its own design once Phases 1 to 5 have taught what the kernel needs.

## Constraints

- **Language and stack:** Rust, iced, wgpu, as today. No new runtime language in the product.
- **Licence:** the repository is LGPL-3.0-or-later; dependencies must be compatible (MPL-2.0, MIT, Apache-2.0). Each new dependency is checked.
- **Browser:** WebGPU with the WebGL2 fallback, no worker threads yet. Phases 0 to 2 (selection, hiding, handles and stretching on meshes) work in the browser too. Parametric editing from Phase 3 on is desktop-first, behind `cfg` gates like IFC today; the browser follows later, with Web Workers or wasm threads, in a design of its own. (Decided by the project owner.)
- **Where 120 fps applies:** the criterion is hard on a reference desktop PC with a discrete GPU, fixed in Phase 0. On integrated graphics the rule is no regression against the Phase 0 baseline. If the baseline misses 120 fps on the reference PC, Phase 0 includes the renderer work needed to reach it before Phase 1 starts. (Decided by the project owner.)
- **The validator branch** is the model for the workspace split. `bim-core` does not wait for it: it sets up the Cargo workspace itself and builds on `ifc-lite-core` directly. When the validator branch is merged, the entity graph moves to one shared crate that both use.
- **Repository rules:** no names of competing commercial software in code, docs or commits; content in English, with the Dutch README kept in step.

## Calls made in writing this design

These were settled by the author of this document, not by the project owner, and are open to correction:

- Saving an opened IFC writes its retained entity graph back with only the edited elements changed, in the file's own schema (Phase 3).
- Stretch moves vertices beyond a plane; it never scales (Phase 2).
- Rectangular openings come along to Phase 3 through the direct construction, so walls with windows and doors are editable before booleans exist.
- Thickness of a wall with a type-shared layer set is not editable until Phase 6.
- Unedited mesh elements keep f32 vertices relative to an f64 model origin.
- Phase 5 is split into GPU residency (5a) and a paged document (5b).
- `bim-core` sets up the workspace itself instead of waiting for the validator branch.

## Left to the builder

- The exact layout of the edit layer and when it is folded back.
- Tile size in Phase 5 (Open BIM Studio has not fixed one either).
- Whether undo snapshots are whole-document copies (cheap with immutable elements) or diffs.
- How the per-element texture is sized for models above the texture limits (split per layer or a second texture).
- The order of the parameter handles within Phase 3, as long as wall length comes first.
- Naming of crates, commands and shortcuts, following the existing command list in `src/shell.rs`.
