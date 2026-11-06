# Structural analysis models: viewing, node matching and clash

**Status:** proposal, 2026-11-06. Nothing in this document is built yet. Branch `feat/structural-model` (from `main` at `0e64343`). It builds on PR #91 (`feat/selection-visibility`) and slots into PR #79 (`codex/rust-bim-validator`) once that is rebased.

**Extends:** [`2026-11-06-bim-modeller-design.md`](2026-11-06-bim-modeller-design.md) (called *the modeller design* below): document vs display, `bim-core`, f64 kernel, stable ids, per-element GPU flags, 120 fps, wasm. Where the two disagree, the modeller design wins and this document is corrected. The pieces this design pulls forward from the modeller design are listed under *Calls made in writing this design*.

**Naming.** Product names appear here only as names of file formats the app opens, as the README does: IFC, Revit (`.rvt`), and the FE-package format that a separate clean-room project reads. That project's crate is called `rfem-dll` in the request; code, commands, ABI symbols and UI in this repository use format-neutral names (`structural`, `sam` for *structural analysis model*, `fe_dll`).

## Intent

Structural engineers exchange two kinds of model of the same building. The physical model has columns, beams and slabs as solids. The analysis model has nodes, member axes, supports, hinges, cross-sections, load cases, loads and results. Today the viewer opens only the first kind well. An IFC4 analysis model opens as ribbons 0.2 file units wide and zero-thickness faces, without supports, loads, results or profiles. A file with only nodes and axes does not open at all.

The aim is that an engineer opens an analysis model from IFC4, from Revit or from an FE package, and reads it the way they read it in their own tools:

- supports as recognisable symbols (fixed, pinned, roller, sliding, spring), also when the source only says "hinge" or "roller" and gives no degrees of freedom;
- member end hinges, member axes, and, on request, members as **complete solids** with their profile placed at the right cardinal point and y/z eccentricity;
- load cases with load arrows and q-load trapezoids;
- result sets with N, V and M diagrams on the tension side.

On top of that come two checks:

- **node matching:** which nodes of model A correspond to nodes of model B (Revit vs IFC, IFC vs FE package, analysis vs physical) within a coordinate tolerance the engineer sets;
- **clash** in Open BIM Validator, in which an analysis member counts as its full profile solid, eccentricities applied.

All of this lives in one vendor-neutral, headless **structural analysis model** in f64 and SI units. The viewer derives its display from it; the validator checks and clashes it; the clean-room FE reader produces it; the IFC reader fills it.

We visualise and check. We do not compute. Every number shown comes from the file, apart from geometry (section outlines, section properties, member frames, solids) and comparisons (distances, overlaps).

## Goals

- **Analysis models open everywhere the viewer runs.** Nodes, members, surfaces, supports (point, line and surface), releases, sections, load cases, combinations, actions and results become document objects with stable ids. A file with only analysis objects opens and fits to view. The `.samjson` exchange file opens on the desktop and in the browser; IFC, Revit and FE-package import stay desktop-only, as IFC is today.
- **Engineering symbols at 120 fps.** Supports, hinges, nodes, load arrows and diagrams are drawn on the GPU as instanced, screen-sized 3D glyphs, wide lines and fills. Switching load case or result set, or dragging a scale slider, never stalls the camera.
- **Members as solids.** Each member sweeps its section along its axis, with cardinal point, section eccentricity and rigid end offsets applied, giving a closed mesh. Display, section box, measure, picking and clash use that one mesh.
- **Node matching with an adjustable tolerance** between any two node sets, with colour-coded results, deviation vectors, a matched-fraction curve, a table, CSV/JSON/BCF export and MCP tools.
- **Clash in Open BIM Validator** from a shared headless engine; analysis member solids are ordinary clash items; results go to BCF with camera, both components and colours.
- **Everything through MCP.** Each panel action has an MCP tool.
- **Every phase is one mergeable PR**, keeps the 120 fps criterion and keeps the wasm32 build working.

## Non-goals

- **Structural analysis.** No solving, no superposition of results the file does not hold, no design checks, no warping-based shear centres. Section properties from polygon integrals and closed-form thin-walled shear centres are geometry and are allowed.
- **Editing analysis models** (moving nodes, changing releases). Structural data is read-only document data until the modeller design's editing phases reach it.
- **Writing the analysis model back to IFC.** The IFC export stays ReferenceView, mesh-only (`src/export/ifc.rs:346`), until Phase 3's retained-graph save exists. See *Open questions*.
- **FE mesh results on surfaces** (plate force contours). Surface members are drawn; their results come in a later design.
- **Reading FE-package files in this repository.** That happens in the clean room. This repository consumes the C ABI and exchange format defined below, nothing else.
- **A bundled steel profile catalogue.** Dimensions come from the file. See *Open questions*.

## Anti-goals (failure even if it technically works)

- **Structural objects baked into `Layer` triangles as the only truth.** Supports, nodes, loads and diagrams are typed document objects with a derived display, or they cannot be matched, clashed or restyled.
- **CPU work per camera frame.** Tessellating symbols in a canvas, rebuilding diagrams, or copying vectors into `ScenePrimitive` on a camera frame fails the 120 fps rule. Only bounded text goes through canvas.
- **Re-uploading on a selector change.** Switching load case, result component or scale never re-uploads layer buffers.
- **Silent double eccentricity.** Applying a cardinal point and a profile offset that encodes the same thing, without recording it, is a failure even when one sample looks right. Every applied offset is recorded with its source.
- **Vendor conventions leaking into the display.** A member's local z, a moment sign, a reaction sense or a gravity direction is resolved at import into the canonical conventions below. The renderer never knows which program a model came from.
- **A second geometry pipeline.** The clash engine does not sweep its own members; it reads the evaluator output the display draws.
- **Breaking PR #91.** Structural objects are elements: `ElementKey`, `UnitKey::Element`, `Selection`, the units-texture flags and the history apply to them without parallel mechanisms.
- **Breaking the browser.** `bim-structural`, the `.samjson` loader, the renderer paths and the clash engine core compile for wasm32 (WebGPU and WebGL2).

## Where the viewer stands today

File references are to `main` at `0e64343` unless marked #91 or #79.

- **Model.** `Layer` (`src/model.rs:12-34`) is a triangle list (or a point cloud) with `elements: Vec<(u64, u32, u32)>` and JSON props. There is no line, glyph or text primitive. `GridLine` (`model.rs:184-189`, in `ProjectData`) is the only non-mesh 3D object, drawn as a dash-dot `LineList` (`render.rs:590-598`, `datum_lines` at 1041). It is the precedent for structural lines.
- **Analysis-only files cannot open.** `Model::new` drops layers without triangles that are not point clouds (`model.rs:390`), computes bounds from layers only (396-403), and `load_stack` fails with "no geometry found" on an empty model (`src/loaders/mod.rs:299-301`).
- **Origins.** Each loader subtracts its own f64 anchor and keeps it only as text (`src/loaders/ifc.rs:81,112-115,152`). `Model::combine` (`model.rs:339-385`) does not align anchors.
- **Ids and flags (#91).** `ElementKey { model: u32, source: u64 }`; `UnitKey::{Element, LayerRest}` (`src/units.rs:13`); `UNIT_HIDDEN = 1`, `UNIT_SELECTED = 2` in the `w` channel of the `Rgba32Float` units texture (1024 wide, at most 2048 rows, `MAX_UNITS = 2 097 152`); `Selection` of `Pickable::{Element(u64), Layer(usize)}`; box select over `ExplodeIndex` unit bounds; undo of visibility; hidden vertices go to `NOWHERE` in `scene.wgsl`.
- **Renderer.** One MSAA 4x pass (`render.rs:809-958`): lit, points, cut fill, glass, edges (`LineList`, 1 px), datum grids, ground grid. Points are instanced screen-space quads (`vs_point`, `scene.wgsl:173-203`) because WebGPU and WebGL2 have no point size or line width. `PointVertex` carries no `unit`. `Uniforms` (`render.rs:87-95`) and `scene.wgsl:3-20` change together; `params.zw` hold the point size. GPU buffers are immutable per layer, keyed by `Model::id`, uploaded in `upload_model` (579-640).
- **Overlays.** Text exists only in iced `canvas` programs stacked in `App::viewport_view` (`src/app.rs:2149-2159`). None uses `canvas::Cache`.
- **Picking** is ray against triangles (`App::pick` at `app.rs:1236` → `pick_triangle` at 2346). `element_json` (`src/remote.rs:756-790`) fails for ids without triangles.
- **IFC** (`src/loaders/ifc.rs`, desktop only) reads only `result.meshes` from `ifc-lite-processing =25.0.1`. ifc-lite meshes `IfcStructuralCurveMember` as a ribbon of half-width 0.1 file units with an arbitrary frame (no `Axis`, profile, cardinal point or eccentricity; `processors/structural_edge.rs:27`), meshes planar `IfcStructuralSurfaceMember` faces with zero thickness, and rejects point connections, conditions, actions, reactions, results and the analysis model. `ifc-lite-core` exposes the full entity graph with typed slots for every `IfcStructural*` entity; `ProcessingRequest::index(Arc<EntityIndex>)` lets a second pass share the entity index; `ifc-lite-geometry` exposes placement resolution, `ProfileProcessor`, `extrude_profile`, `kernel::broadphase::Bvh`, `kernel::tritri` and `clash_solid::intersection_solid`. All MPL-2.0, already transitive dependencies.
- **Revit.** `src/loaders/rvt.rs` uses display meshes (Revit's saved graphics, justification baked in) and project data only. The reader (`rvt-dll` at `54a2455`, branch `open-research`, licence tag `LicenseRef-Proprietary`) skips classes whose name starts with `Analytical` (`mesh.rs:1113-1118`) and has no typed reader for analytical members, location curves, justification or releases. It has a generic schema-driven object layer (`doc.objects_of_class`, `rvt_query` JSON) and typed column base/top levels (`rvt_model::Column`).
- **Validator (#79, unmerged, 35 commits behind `main`).** Same lib crate with feature `bim-validator`, desktop-only. `crates/bim-validator-core` is mesh-free by design (string-keyed `Graph` on `ifc-lite-core`). `crates/bim-bcf` writes topics with component GUIDs only. No clash code. `select_finding` (`src/validation.rs`) scans props JSON linearly.
- **Bench.** `--bench synthetic:N --throughput --frames 300`. #91 baseline: 100k synthetic elements at 749-912 fps (p95 1.2-1.85 ms), a 194 MB IFC at 350-400 fps, on the reference PC. There is room on the GPU; the risk is CPU work per frame.

## How engineers use it

The data model and the renderer follow from these sessions; each is an exit test in *Phases*.

1. **Read an analysis model.** Open an FE-package file (desktop) or a `.samjson` (anywhere). Member axes show as 2 px lines, supports as 3D symbols at constant screen size, hinges as rings set back from the node. In the *Structural* tab, pick load case `LC3 Wind X`: arrows and trapezoids appear. Pick result `CO2 (ULS)` and component `My`: moment diagrams on the tension side, with min and max labels. Drag the diagram scale. Hover a member: number, section and extremes. Click: properties with section, frame, releases as `000011`, eccentricities and where each came from.
2. **See it as solids.** Toggle *Members: axes / solids / both*. The HEB 200 hangs under the axis at cardinal point 8. Put a section box through a joint; measure from a flange to a physical slab.
3. **Check nodes.** Open the Revit model, then *Add* the IFC analysis model. *Structural → Compare nodes*: A = Revit physical axis ends, B = IFC analysis nodes, tolerance 100 mm, *project to axes* on. Nodes turn green, amber, red, purple; arrows run from A to B; the curve shows 94 % matched at 50 mm and 99 % at 120 mm. Click *Unmatched in B* to isolate them. Export CSV or BCF.
4. **Clash.** In Open BIM Validator, open the building services IFC and the analysis model. *Clash*: *Analysis members (solids)* against *IfcDuctSegment, IfcPipeSegment*, hard clash, tolerance 10 mm, joint overlaps excluded. Run: 37 clashes, grouped by storey. Click one: the pair is isolated, tinted red and amber, a section box is placed around it. Export BCF; the topics open with camera and colours in other BCF tools.

## Architecture

### Crates and modules

| Where | What | Depends on | Targets |
|---|---|---|---|
| `crates/bim-structural` (new, LGPL-3.0-or-later, headless) | Document types, conventions, support and hinge presets, `sam-json` v1 exchange and validation, `sam-check` binary, member frames, section shapes, outlines and properties, cardinal points, insertion resolution, profile sweep, node matching, consistency checks, lazy props JSON, diagram and load geometry in f64 | `glam` (`DVec3`, `DMat3`, `DMat4`, `DAffine2`), `serde`, `serde_json`, `earcutr` (ISC) | all, incl. wasm32 |
| `crates/bim-structural`, feature `ifc` | IFC4 / IFC4X3 / IFC2X3 structural analysis reader; physical-axis node extraction | `ifc-lite-core =25.0.1`, `ifc-lite-geometry =25.0.1` (placement, `ProfileProcessor` at import only) | desktop (`cfg(not(wasm32))` in the root crate, as IFC today) |
| `crates/bim-clash` (new, LGPL, headless) | Clash engine: items, tests, broad and narrow phase, rules, deterministic ids, deviation report | `glam`, `uuid` (v5); feature `exact`: `ifc-lite-geometry` (`tri_tri_intersection`, `intersection_solid`); feature `parallel`: `rayon` | engine on all targets; `exact` and `parallel` on desktop |
| `crates/sam-abi-mock` (new, test-only `cdylib`) | Serves a `.samjson` through the `sam_*` C ABI | `bim-structural` | desktop |
| `src/structural/` (viewer) | `mod.rs` (`StructuralPart`, `StructuralIndex`, props fallback), `display.rs` (f32 instance data derived from the document), `pick.rs`, `labels.rs`, `panel.rs` (Structural tab), `matching.rs` (compare-nodes UI state) | `bim-structural` | all |
| `src/viewport/structural.rs` + `structural.wgsl` | Segment, glyph and fill pipelines, glyph mesh library, `StructUniforms` | wgpu (via iced) | all, WebGL2-safe |
| `src/loaders/sam.rs` | `.samjson` loader (exchange format and test fixtures) | `bim-structural` | all |
| `src/loaders/fe_dll.rs` | FE-package loader over the `sam_*` C ABI, `libloading` as `vertex.rs` does | `bim-structural`, `libloading` (present) | desktop |
| `src/loaders/ifc.rs` (extended) | Shares the `EntityIndex`, runs the structural pass, suppresses ifc-lite ribbons | `bim-structural/ifc` | desktop |
| `src/loaders/rvt.rs` (extended) | Physical-axis node sets; later the analytical model | `rvt-read` (rev bump when needed) | desktop |
| `src/clash.rs` | Feeds `bim-clash` from loaded models, runs jobs, MCP handlers; used by both products | `bim-clash` | desktop first |
| `src/validation.rs`, `src/validation_cli.rs` (#79) | Clash tab, BCF adapter, `--clash` | `bim-clash`, `bim-bcf` | desktop, feature `bim-validator` |

The root `Cargo.toml` gets `[workspace] members = ["crates/bim-structural", "crates/bim-clash", "crates/sam-abi-mock"]`; #79 adds its two crates to the same list on rebase (a one-line conflict). New direct dependencies: `ifc-lite-core` and `ifc-lite-geometry` pinned `=25.0.1` like `ifc-lite-processing` (MPL-2.0, already transitive), `earcutr` (ISC), `uuid` (MIT/Apache-2.0, already in #79). No GPL or AGPL.

**Relation to `bim-core`.** `bim-core` does not exist yet; the modeller design says it sets up the workspace itself in Phase 3. `bim-structural` is written as if it were a module of `bim-core`: f64, SI, ids as `u64`, no GUI or GPU types, nothing from the viewer. When `bim-core` lands, its `Element` gets an `Analytic` representation (vertex, axis, face topology) pointing into these types, and the member solid becomes its derived `Mesh` representation. That is a move, not a rewrite.

**Relation to the validator.** The structural IFC reader is written once, on the slot-based `EntityDecoder`. The validator calls `bim_structural::ifc::read` on the same bytes (one entity-index build on a worker); its string-keyed `Graph` is not extended for structural entities. When the modeller design's shared entity-graph crate exists, the reader moves onto it.

### Data flow

```
 .ifc ──ifc-lite-processing──► meshes (structural ribbons dropped) ────────┐
   │   (one shared Arc<EntityIndex>)                                        │
   └──bim_structural::ifc::read──► AnalysisModel (f64, SI, world metres) ──┤
 .samjson ──bim_structural::exchange::read ─────────────────────────────────┤
 FE file ──clean-room DLL (sam_* ABI: sam-json + result blocks) ────────────┤
 .rvt ──rvt.rs (physical axes, later analytical) ───────────────────────────┤
                                                                            ▼
                     Loaded { layers, structural: Vec<AnalysisModel>, origin }
                                                                            ▼
                     Model { layers, structural: Vec<StructuralPart>, origins }
                         │                     │                     │
            solid::sweep (worker)     display::build (worker)   matching / clash (worker)
            → "Analysis members" Layer  → GpuStructural          → reports, tints, BCF, MCP
```

### Document and display

- **Document:** `bim_structural::AnalysisModel` in an `Arc`, one per analysis model in a source file, stored on `Model` inside a `StructuralPart`. Never touched on the frame path.
- **Display:** `src/structural/display.rs` derives a `StructuralDisplay` per model on the load worker: f32 instance arrays relative to the part's origin (axes, nodes, supports, hinges, rigid offsets, line-support hatches). It is published next to the model (`App.structural_display: FxHashMap<u64 /*Model::id*/, Arc<StructuralDisplay>>`) and handed to `ScenePrimitive` as one `Arc`, not stored inside the document. Load and result geometry is derived on demand (see *Rendering*).
- **Member solids are a mesh layer.** The sweep output (f64, closed, outward) becomes one ordinary `Layer` named `Analysis members` whose `elements` use the member ids. Section cut fill, measure snapping, triangle picking, box select, the exploded view and the clash engine therefore use the drawn solid: the modeller design's "no second geometry pipeline".

### Identity, units and flags (fits PR #91)

- **Ids.** Every structural object has `id: u64`, unique within its source model and stable across reloads:
  - IFC: the express id of the defining entity (`IfcStructuralPointConnection`, `IfcStructuralCurveMember`, `IfcStructuralSurfaceMember`, `IfcStructuralCurveConnection`, an action, a load group, a result group). A node that exists only as a shared `IfcVertexPoint` uses the vertex's express id. The suppressed ribbon of a member had the member's express id, so nothing collides.
  - `sam-json`, the FE DLL and derived sets: `sam_id(kind, number) = (kind as u64) << 56 | number`, with `kind` from `IdKind` below and `number` the source number. Express ids are below 2³², so the two schemes never collide with physical elements of the same file.
  - Revit: the element id of the analytical element; for physical-axis sets, derived nodes use `sam_id(Node, n)` and axes reuse the physical element id (the axis is then a second representation of that element).
- **Elements.** In the viewer each structural object is `ElementKey { model, source: id }` and `Pickable::Element(id)`. Selection, hide, isolate, box select, undo, `find_elements` and properties work with no new variant.
- **Units.** Only nodes, members and surface members get units in `ExplodeIndex` (`src/explode.rs:51` gains a second input `StructuralPart::units()`, bounds = point or segment box, storey = nearest level below). Everything else (supports, hinges, line supports, rigid offsets, loads, diagrams, labels) carries the unit of the item it decorates, so hiding a member hides its loads, hinges and diagram, and the exploded view moves them together. 10⁵ members and 10⁵ nodes use about 10 % of `MAX_UNITS`.
- **Addressable decorations.** Actions, line supports, load cases and result sets have ids and properties but no unit of their own. `StructuralIndex::unit_key(id)` maps a decoration to its target's unit, so selecting a load arrow selects the load (its properties show) and highlights its member. When a member has a solid, the triangles and the axis share one unit.
- **Tint bits.** Node-matching and clash status colours go into the units texture: bits 4-7 of `w` hold a palette index (`UNIT_TINT_SHIFT = 4`, 0 = none), next to #91's `UNIT_HIDDEN` and `UNIT_SELECTED` (bits 0-1; bits 2-3 stay reserved for hovered and edited, as the modeller design plans). `w` is an f32, exact for integers up to 2²⁴. A 16-entry palette is a small separate uniform buffer bound at `@group(0) @binding(2)`, rewritten only when the palette changes. `fs_lit`, `fs_glass` and the structural shaders mix towards the tint. Recolouring 10⁵ items, or tinting a physical duct in a clash, is one texture write. This is its own PR after #91 merges.
- **Properties are lazy.** `StructuralIndex::props_json(id)` builds a flat JSON object (`Class`, `Number`, `Section`, `Material`, `Start node`, `End node`, `Length`, `Support`, `Releases`, `Cardinal point`, `Offsets`, `Linked element`, ...) when asked. `Model::element_props`, `element_json` and `find_elements` fall back to it when an id has no triangle range. Nothing is written into `Model::props` per object at load. `Model::element_bounds(id)` consults the structural index before the layers, for `zoom_to`, `get_element` and validation framing.

### Coordinates and origins

Matching and clash need models in one frame, in f64. That needs a structured origin, which the modeller design plans for Phase 3. This design pulls one small part forward:

- `Loaded.origin: Option<DVec3>` and `Model.origins: Vec<DVec3>` indexed by `ElementKey.model`, filled by every loader that subtracts an anchor (IFC, Revit, `.samjson`, FE DLL) from the value it already subtracts. Z-up metres.
- `AnalysisModel` positions are f64 world metres of the source file, **without** subtracting the anchor. The display subtracts `origins[model]` once, in f64, before the cast to f32.
- Matching and clash work in world coordinates (`origin + local` for mesh layers, the f64 document for structural objects), so they are correct across models even while the display is not aligned.
- **Display alignment on Add:** when an added model's origin differs from the first model's by less than 10 km, `Model::combine` translates its f32 positions by the f64 difference (rounded once, error under 1 mm). Above 10 km the models stay unaligned, as today, and the app says so. Phase 3's frames replace this.

## The structural model (`crates/bim-structural`)

### Conventions (normative for every importer and for the FE DLL)

- **Units:** SI base units in f64: m, N, N·m, N/m, N/m² (Pa), rad, K. Translational spring N/m, rotational spring N·m/rad, line-support springs N/m², surface-support springs N/m³. kN, kNm and mm exist only in the UI.
- **Global frame:** right-handed, Z up, gravity −Z. A source with gravity +Z is rotated 180° about X, `(x, y, z) → (x, −y, −z)`, never mirrored; the rotation is recorded in `SourceInfo.global_transform`.
- **Member local frame:** x from start to end (curve tangent for curved members). z is the resolved reference vector made perpendicular to x: `z = normalize(z_ref − (z_ref·x) x)`. y = z × x. Profile u (width) maps to local y and profile v (height) to local z, as in IFC. Note that member axes x, y, z correspond to z, x, y of a profiled `IfcBeam`, whose extrusion axis is z.
- **Internal forces:** the positive cut face is the face whose outward normal is +x. Forces on it act along +x, +y, +z (N > 0 is tension); moment vectors point along +x, +y, +z. `σ = N/A + My·z/Iy − Mz·y/Iz`, so +My puts tension on the +z fibre and +Mz on the −y fibre. A source with the opposite Mz sign is converted at import.
- **Reactions:** the force the support exerts on the structure, in the support frame.
- **Stations:** absolute metres from the member's start node, along the analytical member between its end nodes (rigid end offsets do not shift stations). A jump at a point load or hinge is two stations with the same x, left value first.

### Type sketch

```rust
pub type Id = u64;

#[repr(u8)]
pub enum IdKind { Node = 1, Member = 2, Surface = 3, Section = 4, Material = 5,
                  LineSupport = 6, LoadCase = 7, Combination = 8, Action = 9,
                  ResultSet = 10, Opening = 11 }

pub fn sam_id(kind: IdKind, number: u64) -> Id { (kind as u64) << 56 | (number & ((1 << 56) - 1)) }

pub struct AnalysisModel {
    pub id: Id,
    pub name: String,
    pub dimension: Dimension,            // Space3d | Plane2d { normal: DVec3 }
    pub kind: ModelKind,                 // Analytical | PhysicalAxes (derived from physical elements)
    pub source: SourceInfo,
    pub shared_frame: DMat4,             // IFC SharedPlacement in world metres; identity otherwise
    pub nodes: Vec<Node>,
    pub members: Vec<Member>,
    pub surfaces: Vec<SurfaceMember>,
    pub line_supports: Vec<LineSupport>,
    pub sections: Vec<Section>,
    pub materials: Vec<Material>,
    pub load_cases: Vec<LoadCase>,
    pub combinations: Vec<Combination>,
    pub actions: Vec<Action>,            // sorted by load case, for draw ranges
    pub results: Vec<ResultSet>,
    pub links: Vec<PhysicalLink>,
    pub diagnostics: Vec<Diagnostic>,
    index: FxHashMap<Id, ItemRef>,       // id -> (kind, slot)
}

pub struct SourceInfo {
    pub format: String,                  // "IFC4", "IFC2X3", "sam-json/1", "fe-dll/1", "rvt-derived"
    pub file: String,
    pub application: Option<String>,
    pub global_transform: DMat4,         // applied to reach the Z-up global frame
    pub conventions: Conventions,        // what the source used; already normalised away
    pub eccentricity_policy: EccentricityPolicy,
}

pub struct Conventions {
    pub gravity_was: AxisSign,           // NegZ | PosZ
    pub mz_sign_was: MzSign,             // TensionMinusY (canonical) | TensionPlusY
    pub reactions_were: ReactionSense,   // OnStructure (canonical) | OnSupport
    pub result_signs: SignStatus,        // Verified | AssumedCanonical
    pub local_z_rule: Option<String>,    // how the source defined its default member frame
}

pub struct Node {
    pub id: Id,
    pub number: String,                  // source numbering, kept as text
    pub p: DVec3,                        // world metres
    pub frame: Option<DMat3>,            // skewed support / condition coordinate system
    pub support: Option<Support>,
    pub origin: NodeOrigin,              // Analytical | DerivedFromPhysical { element: Id, approximate: bool }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Dof {
    Fixed,                               // support: restrained; member end: connected
    Free,                                // support: free; member end: released
    Spring(f64),                         // SI stiffness, sign kept as given (negative flagged)
    Nonlinear { kind: NonlinearKind, base: f64 },
}
pub enum NonlinearKind { TensionOnly, CompressionOnly, Failure { limit: f64 },
                         Slippage { gap: f64 }, Diagram(Vec<(f64, f64)>) }
pub type Dofs = [Dof; 6];                // ux uy uz rx ry rz

pub struct Support {
    pub dofs: Dofs,
    pub frame: DofFrame,                 // Global | NodeLocal | MemberLocal(Id)
    pub named: Option<String>,           // source name when the source gave a type, not DOFs
    pub preset: SupportPreset,           // display hint, from support::classify(dofs)
}
pub enum SupportPreset { Fixed, Pinned, Roller { free: [bool; 3] }, Sliding { free: [bool; 3] },
                         Spring, Custom }

pub struct Member {
    pub id: Id,
    pub number: String,
    pub kind: MemberKind,                // Beam | Truss | Cable | TensionOnly | CompressionOnly | Rigid | Spring
    pub nodes: Vec<Id>,                  // start, intermediate..., end
    pub geometry: Curve,                 // Line | Arc { mid: DVec3 } | Polyline(Vec<DVec3>)
    pub z_ref: DVec3,                    // resolved local z reference, authoritative
    pub orientation: Orientation,        // IfcAxis | Beta { angle, rule } | RefNode(Id) | RefVector(DVec3) | Default
    pub section: (Id, Option<Id>),       // start, end (taper)
    pub section_rotation: f64,           // extra rotation of the profile about x, rad (rare)
    pub insertion: Insertion,
    pub releases: [Option<Hinge>; 2],    // member local; None = rigid
    pub material: Option<Id>,
    pub links: Vec<Id>,                  // physical element ids in the same file
}

pub struct Hinge { pub dofs: Dofs, pub named: Option<String> }

/// The independent offset mechanisms, kept apart so none is applied twice.
pub struct Insertion {
    pub cardinal_point: u8,              // 0 = none, 1..=19 (IFC4 IfcCardinalPointReference)
    pub profile_position: DAffine2,      // the profile definition's own Position (u, v), as read
    pub profile_offsets: Option<[DVec2; 2]>, // IfcMaterialProfileWithOffsets, start and end
    pub eccentricity: DVec2,             // member-wide (ey, ez) in local axes; the profile moves by +e
    pub end_offsets: [DVec3; 2],         // rigid offsets node -> member end
    pub end_offset_frame: OffsetFrame,   // MemberLocal | Global
}

pub enum EccentricityPolicy {
    CardinalCancelsPosition,             // default for IFC
    Additive,                            // offsets add up, when a source says so
    Explicit,                            // the producer reduced everything to `eccentricity` (FE DLL default)
}

pub struct SurfaceMember {
    pub id: Id, pub number: String,
    pub kind: SurfaceKind,               // Plate | Membrane | Shell | Rigid | LoadTransfer
    pub boundary: Vec<Id>,               // node loop
    pub openings: Vec<Vec<Id>>,
    pub thickness: Thickness,            // Uniform(f64) | Linear([(DVec3, f64); 3]) | Unknown
    pub offset_z: f64,                   // mid-plane offset from the reference surface
    pub x_dir: Option<DVec3>,
    pub material: Option<Id>,
    pub links: Vec<Id>,
}

pub struct LineSupport {                 // IfcStructuralCurveConnection / SurfaceConnection edges
    pub id: Id, pub number: String,
    pub along: SupportCarrier,           // Member(Id) | SurfaceEdge { surface: Id, from: Id, to: Id } | Polyline(Vec<DVec3>)
    pub dofs: Dofs,                      // springs per length
    pub frame: DofFrame,
    pub named: Option<String>,
}

pub struct Section {
    pub id: Id,
    pub name: String,                    // "HEB 200", as given
    pub shape: Shape,
    pub props: SectionProps,             // computed from the outline unless imported
    pub props_source: PropsSource,       // Computed | Imported | ShearCentreApproximate
    pub material: Option<Id>,
}

pub enum Shape {                         // dimensions in metres, origin at the bounding-box centre (IFC)
    I { h: f64, b: f64, tw: f64, tf: f64, r: f64, slope: f64 },
    AsymI { h: f64, bt: f64, bb: f64, tw: f64, tft: f64, tfb: f64, r: f64 },
    U { h: f64, b: f64, tw: f64, tf: f64, r1: f64, r2: f64, slope: f64 },
    L { h: f64, b: f64, t: f64, r1: f64, r2: f64 },
    T { h: f64, b: f64, tw: f64, tf: f64, r: f64 },
    C { h: f64, b: f64, t: f64, lip: f64, r: f64 },
    Z { h: f64, b: f64, tw: f64, tf: f64, r: f64 },
    Rect { b: f64, h: f64 },
    Circle { d: f64 },
    Chs { d: f64, t: f64 },
    Rhs { b: f64, h: f64, t: f64, r_out: f64, r_in: f64 },
    Polygon { outer: Vec<DVec2>, holes: Vec<Vec<DVec2>> },
    Composite(Vec<(Shape, DAffine2)>),
    Unknown,                             // name only; drawn as a placeholder (see Profile solids)
}

pub struct SectionProps { pub area: f64, pub centroid: DVec2, pub shear_centre: DVec2,
                          pub iy: f64, pub iz: f64, pub iyz: f64, pub it: Option<f64>,
                          pub bbox: [DVec2; 2] }

pub struct Material { pub id: Id, pub name: String, pub e: Option<f64>, pub g: Option<f64>,
                      pub nu: Option<f64>, pub density: Option<f64>, pub alpha_t: Option<f64> }

pub struct LoadCase {
    pub id: Id, pub number: String, pub name: String,
    pub action_type: ActionType,         // Permanent | Variable | Accidental | User(String)
    pub action_source: String,           // IFC enum text: DEAD_LOAD_G, WIND_W, ...
    pub self_weight: Option<DVec3>,      // factors
    pub coefficient: f64,
}

pub struct Combination {
    pub id: Id, pub number: String, pub name: String,
    pub purpose: Purpose,                // Uls | Sls | Als | User(String)
    pub terms: Vec<(LoadRef, f64)>,      // LoadRef = Case(Id) | Combination(Id); shown, never evaluated
}

pub struct Action {
    pub id: Id,
    pub load: LoadRef,
    pub target: Target,                  // Node(Id) | Member { id, span: Option<(f64, f64)> } | Surface(Id) | Free(Vec<DVec3>)
    pub frame: LoadFrame,                // Global | Local | GlobalProjected
    pub kind: Load,
    pub destabilising: bool,
}

pub enum Load {
    NodalForce { f: DVec3, m: DVec3 },
    MemberPoint { x: f64, f: DVec3, m: DVec3 },
    MemberDistributed { dir: DVec3, moment: bool, dist: Distribution },
    Surface { dir: DVec3, dist: SurfaceDistribution },  // Uniform(p) | Bilinear([p; 3]) | Sampled(Vec<(DVec2, f64)>)
    Temperature { dt: f64, dt_y: f64, dt_z: f64 },
    Displacement { u: [Option<f64>; 6] },
}

pub enum Distribution {                  // intensity as given in its frame; x absolute metres
    Uniform(f64),
    Linear { q1: f64, q2: f64 },
    Trapezoidal { a: f64, b: f64, q1: f64, q2: f64 },
    Polygonal(Vec<(f64, f64)>),
    Parabolic { peak: f64 },
    Sine { peak: f64 },
}

pub struct ResultSet {
    pub id: Id, pub number: String, pub name: String,
    pub for_: ResultFor,                 // Case(Id) | Combination(Id) | Envelope(String)
    pub theory: Theory,                  // First | Second | Third | FullNonlinear
    pub linear: bool,
    pub available: Vec<TableKind>,       // MemberForces | MemberDeformations | NodeDisplacements | Reactions | Envelope
    pub payload: Lazy<ResultPayload>,    // fetched on first use through a ResultSource
}

pub struct ResultPayload {
    pub member_forces: Option<MemberTable>,        // N Vy Vz Mt My Mz, member local, canonical
    pub member_deformations: Option<MemberTable>,  // ux uy uz rx ry rz, member local
    pub node_displacements: Option<NodeTable>,     // u, phi, global frame
    pub reactions: Option<NodeTable>,              // F, M, support frame, on the structure
    pub envelope: Option<EnvelopeTable>,           // max/min with governing set, only as given
}

/// Columnar, so 10^5 members x 20 stations x 6 components stays in a few flat Vecs.
pub struct MemberTable {
    pub member: Vec<Id>,
    pub offsets: Vec<u32>,               // len = member.len() + 1
    pub x: Vec<f64>,
    pub comps: [Vec<f64>; 6],
}
pub struct NodeTable { pub node: Vec<Id>, pub comps: [Vec<f64>; 6] }

pub trait ResultSource: Send + Sync {
    fn fetch(&self, set: Id, kind: TableKind) -> Result<ResultPayload, String>;
}

pub struct PhysicalLink { pub analytical: Id, pub physical: Id, pub source: LinkSource }
// LinkSource = IfcRelAssignsToProduct | RevitAssociation | Matched { distance: f64 }

pub struct Diagnostic { pub object: Option<Id>, pub code: &'static str, pub message: String }
```

`Diagnostic` codes list everything an importer skipped, guessed or converted, for example `ifc2x3-rigid-minus-one`, `default-local-z`, `implied-releases-pin-joined`, `double-eccentricity-suppressed`, `profile-with-offsets`, `shear-centre-approximated`, `unknown-profile`, `sign-mz-flipped`, `result-signs-assumed`, `negative-spring`, `named-support-expanded`. The *Model notes* list in the panel, `get_structural_issues` and the validator report show them.

### Support and hinge presets

`bim_structural::support::classify(&Dofs, Dimension) -> SupportPreset` is the one place that turns DOFs into a symbol. The reverse, `support::expand(name) -> Option<Dofs>`, serves sources that give only a named type ("this is a hinge or a roller"): the importer expands the name through the documented table below, keeps the source name in `named`, and adds `named-support-expanded`. A name not in the table becomes `Custom` with all DOFs `Free` and an `unknown-support-name` diagnostic, never a guess.

DOF codes in the UI and in properties are six characters `ux uy uz rx ry rz`: `1` restrained (support) or connected (hinge), `0` free or released, `s` spring, `n` nonlinear.

| Name (case-insensitive) | Support DOFs | Notes |
|---|---|---|
| `fixed`, `clamped` | `111111` | |
| `pinned`, `hinged`, `hinge` | `111000` | |
| `roller` | `001000` | free in both horizontal directions of the support frame |
| `roller_x`, `roller_y` | `011000`, `101000` | free along the named axis only |
| `sliding_x`, `sliding_y` | `011111`, `101111` | guided: rotations fixed |
| `free` | `000000` | |

| Name | Member-end DOFs | Notes |
|---|---|---|
| `rigid` | `111111` | no release |
| `hinge`, `moment_hinge` | `111100` | ry and rz released |
| `hinge_y`, `hinge_z` | `111101`, `111110` | one bending axis released |
| `torsion` | `111011` | rx released |
| `axial` | `011111` | ux released |

`IfcStructuralCurveMember.PredefinedType = PIN_JOINED_MEMBER` without explicit end conditions gets `hinge` at both ends with `implied-releases-pin-joined`; explicit conditions win.

### Section geometry

- `section::outline(&Shape, chord_tol = 0.1 mm, max_per_quarter = 8) -> Outline2` tessellates in profile `(u, v)` with the origin at the bounding-box centre (IFC parametric profiles). Fillets within the chord tolerance; sloped flanges from `slope`; hollow shapes and `Polygon` holes as inner loops; `Composite` applies its transforms. Outer loops counter-clockwise, holes clockwise.
- `section::props(&Outline2) -> SectionProps`: exact Green's-theorem integrals (A, centroid, Iy, Iz, Iyz). Shear centre: the centroid for doubly symmetric shapes; closed forms for L (leg intersection), T (flange/web intersection) and U/C (`e = 3·b'²·tf / (6·b'·tf + h'·tw)` from the web centreline, centreline dimensions); otherwise imported, else the centroid with `shear-centre-approximated`. Imported properties win for labels, never for geometry.
- `section::cardinal_anchor(&SectionProps, cp) -> DVec2` for IFC4 `IfcCardinalPointReference` 1-19 only: 1-9 points of the bounding box (bottom row 1-3, mid-depth 4-6, top 7-9; within a row left, centre, right). As IFC defines it, seen looking along the extrusion direction (into the drawing), *left* is +u and *top* is +v, so 1 is (u_max, v_min) and 9 is (u_min, v_max). 10 the centroid, 11-14 the centroid projected onto the bottom, left, right and top edges, 15 the shear centre, 16-19 the shear centre projected likewise. 0 means no anchor. Any other value is refused with a diagnostic and treated as 0.

## Reading IFC4 structural entities (`bim-structural`, feature `ifc`)

### Loader flow (`src/loaders/ifc.rs`)

```rust
let index = Arc::new(ifc_lite_core::build_entity_index(&bytes));
let result = run_on_geometry_pool(|| process(header,
    &ProcessingRequest::full_pass().instancing(false).index(index.clone()), &mut ()));
let anchor = result.frame.to_world([0.0; 3]);                    // as today (ifc.rs:81)
let sam = bim_structural::ifc::read(&bytes, index, &IfcFrame::from(&result.frame))?;
```

- `read` returns `Ok(vec![])` almost for free when the index holds no `IFCSTRUCTURAL*` record (a type-name lookup). There is no second scan of the file.
- On success with at least one member or surface, the loader drops meshes whose `IfcType` is `IfcStructuralCurveMember`, `IfcStructuralCurveMemberVarying`, `IfcStructuralSurfaceMember` or `IfcStructuralSurfaceMemberVarying`, adds the evaluated `Analysis members` layer (S6) and sets `Loaded.structural` and `Loaded.origin = anchor`. Non-planar surface faces the reader cannot handle keep their ifc-lite mesh, with a diagnostic.
- On failure the loader keeps today's ribbons, logs the error in project info, and still opens the file.

**Coordinates.** `IfcFrame` takes its world mapping from `result.frame` (`world_mapper`) so that analytical nodes land exactly where meshes land: unscaled placement (`placement::resolve_object_placement`, column-major, file units) times `decoder.length_unit_scale()`, mapped by the same function the meshes use. The `alignment.ifc` fixture pins this to 1e-6 m.

### Reading steps (typed slots from `ifc_lite_core::slot::*`)

1. **Units.** `decoder.length_unit_scale()`; `ProjectUnits::resolve(..).unit_for_measure` for `FORCEMEASURE`, `LINEARFORCEMEASURE`, `PLANARFORCEMEASURE`, `MOMENTMEASURE`, `LINEARMOMENTMEASURE`, `LINEARSTIFFNESSMEASURE`, `ROTATIONALSTIFFNESSMEASURE`, `THERMODYNAMICTEMPERATUREMEASURE`, `PLANEANGLEMEASURE`. Every value is converted to SI where it is read.
2. **Inverse maps.** One pass with `find_by_type` over `IFCRELASSIGNSTOGROUP`, `IFCRELASSIGNSTOGROUPBYFACTOR`, `IFCRELCONNECTSSTRUCTURALMEMBER`, `IFCRELCONNECTSWITHECCENTRICITY`, `IFCRELCONNECTSSTRUCTURALACTIVITY`, `IFCRELASSIGNSTOPRODUCT` and `IFCRELASSOCIATESMATERIAL` into `FxHashMap<u32, SmallVec<[u32; 4]>>` keyed by the related id.
3. **Analysis models.** Each `IfcStructuralAnalysisModel` (`PREDEFINED_TYPE`, `ORIENTATION_OF2_D_PLANE`, `LOADED_BY`, `HAS_RESULTS`, `SHARED_PLACEMENT`) becomes an `AnalysisModel`; its items come from `IfcRelAssignsToGroup`. Items in no model go into one named "Unassigned". `SharedPlacement` becomes `shared_frame`.
4. **Nodes.** From each item's `IfcTopologyRepresentation` ('Reference'): each `IfcVertexPoint → IfcCartesianPoint` becomes a node, keyed by the vertex's express id. Connectivity is by vertex identity, never by coordinates. An `IfcStructuralPointConnection` names its vertex node and gives `number` (`Name`), `ConditionCoordinateSystem` (→ `Node.frame`, relative to the shared frame) and `AppliedCondition`.
5. **Conditions.** `IfcBoundaryNodeCondition` through `decoder.slot(SchemaSlot)`, because IFC2X3 and IFC4 layouts differ:
   - IFC4: `Typed{IFCBOOLEAN, .T.}` → `Fixed`, `.F.` → `Free`, `Typed{IFCLINEARSTIFFNESSMEASURE | IFCROTATIONALSTIFFNESSMEASURE, v}` → `Spring(v·scale)`; a negative value is kept and flagged `negative-spring`.
   - IFC2X3: a real `-1.0` → `Fixed` (`ifc2x3-rigid-minus-one`), `0` → `Free`, other values → `Spring`.
   - Missing (`$`) → `Free`.
   - `IfcBoundaryNodeConditionWarping` is kept as a property.
   - `IfcBoundaryEdgeCondition` and `IfcBoundaryFaceCondition` on `IfcStructuralCurveConnection` / `IfcStructuralSurfaceConnection` become `LineSupport` (per-length or per-area stiffness), carried by the member or surface edge they share topology with.
6. **Members.** `IfcStructuralCurveMember(Varying)` with its 'Edge' topology: start and end vertices; an `IfcEdgeCurve` with non-line geometry becomes `Arc` (trimmed `IfcCircle`) or `Polyline` (sampled within the chord tolerance). `PREDEFINED_TYPE` → `MemberKind` (and implied releases for `PIN_JOINED_MEMBER`). `AXIS` (slot 8) is a direction in the item's object coordinate system, which IFC requires to be the shared placement; it is rotated by that placement into world and resolved to `z_ref` (`orientation = IfcAxis`). Missing `AXIS` (IFC2X3) uses the default rule: y horizontal and z upward in the vertical plane for non-vertical members; for members within 1e-3 rad of vertical, y along global Y. Recorded with `default-local-z`.
7. **Releases.** `IfcRelConnectsStructuralMember.AppliedCondition` gives the hinge at the end whose vertex is the connection's vertex (vertex identity, falling back to the nearer end within 1e-6 m). `TRUE` = connected, `FALSE` = released. `ConditionCoordinateSystem` is relative to the member frame. `AdditionalConditions` map to `Nonlinear` (`IfcFailureConnectionCondition` → `Failure`, `IfcSlippageConnectionCondition` → `Slippage`).
8. **Axis eccentricity.** `IfcRelConnectsWithEccentricity.ConnectionConstraint` (`IfcConnectionPointEccentricity`) becomes `end_offsets` in member axes, from the connection point to the member point. When both the two points and `EccentricityInX/Y/Z` are given and disagree, the point distance wins (IFC rule), with a diagnostic.
9. **Sections.** `IfcRelAssociatesMaterial → IfcMaterialProfileSetUsage` (`FOR_PROFILE_SET`, `CARDINAL_POINT`), or `IfcMaterialProfileSetUsageTapering` for start and end sections. Parameterised profile defs (`IfcIShapeProfileDef`, `IfcAsymmetricIShapeProfileDef`, `IfcUShapeProfileDef`, `IfcLShapeProfileDef`, `IfcTShapeProfileDef`, `IfcCShapeProfileDef`, `IfcZShapeProfileDef`, `IfcRectangleProfileDef`, `IfcRectangleHollowProfileDef`, `IfcCircleProfileDef`, `IfcCircleHollowProfileDef`) are read slot by slot into `Shape`, in metres. Everything else (arbitrary, derived, composite, rounded rectangle) goes through `ProfileProcessor::process` **at import** and is stored as `Shape::Polygon` (or `Composite`), so the document never refers back to the IFC graph and sweeps on every target. The profile's `Position` becomes `insertion.profile_position`. `IfcMaterialProfileWithOffsets.OffsetValues` become `profile_offsets` with `profile-with-offsets`, until real exports confirm their meaning. The material name becomes `Material`.
10. **Surfaces.** Planar 'Face' topology gives a boundary loop of nodes and openings; `Thickness` (slot 8); `IfcMaterialLayerSetUsage` offset → `offset_z`; `PredefinedType` → `SurfaceKind`.
11. **Load groups.** `IfcStructuralLoadCase` / `IfcStructuralLoadGroup` with `PredefinedType = LOAD_CASE` → `LoadCase` (`ActionType`, `ActionSource`, `Coefficient`, `SelfWeightCoefficients`); `LOAD_COMBINATION` → `Combination` with factors from `IfcRelAssignsToGroupByFactor` times the group `Coefficient`; `LOAD_GROUP` is a sub-grouping kept as a property.
12. **Actions.** `IfcStructuralPointAction`, `IfcStructuralCurveAction` and its IFC2X3 form `IfcStructuralLinearAction`, `IfcStructuralSurfaceAction` and `IfcStructuralPlanarAction`, attached to items by `IfcRelConnectsStructuralActivity` and to load groups by group assignment. `AppliedLoad` → `Load` (single force, linear force, planar force, single displacement, temperature). `GlobalOrLocal` and `ProjectedOrTrue` → `LoadFrame` (projected only with global directions). The full `IfcStructuralCurveActivityTypeEnum` is mapped: `CONST` → `Uniform`, `LINEAR` → `Linear`, `POLYGONAL` and `DISCRETE` → `Polygonal` from `IfcStructuralLoadConfiguration` (ascending locations), `EQUIDISTANT` → `Polygonal` with n+1 implicit stations (IFC does not allow it for actions; accepted with a diagnostic), `PARABOLA` → `Parabolic`, `SINUS` → `Sine`, `USERDEFINED`/`NOTDEFINED` → `Polygonal` if a configuration exists, else skipped with a diagnostic. An action on part of an item reads its own topology into `span` or `Free`. Intensities are stored as given in their frame; the display converts projected loads (`q_true = q_proj · sqrt(1 − (t·d)²)`, t the member tangent, d the load direction).
13. **Results.** `HasResults → IfcStructuralResultGroup` (`TheoryType`, `ResultForLoadGroup`, `IsLinear`) → `ResultSet`. `IfcStructuralPointReaction` with a `SingleForce` → reactions; with a `SingleDisplacement` → node displacements. `IfcStructuralCurveReaction` with an `IfcStructuralLoadConfiguration` (or `EQUIDISTANT` expanded to n+1 stations) → `MemberTable`, mapping local `ForceX → N`, `ForceY → Vy`, `ForceZ → Vz`, `MomentX → Mt`, `MomentY → My`, `MomentZ → Mz`. IFC does not state the internal-force sign convention, so the set gets `result_signs = AssumedCanonical` and `result-signs-assumed` until reference exports confirm it (S8 gate). Large files keep curve reactions as offsets into the source until a set is opened (`Lazy`).
14. **Links.** `IfcRelAssignsToProduct` from an analysis item to an `IfcBeam`, `IfcColumn`, `IfcMember`, `IfcSlab`, `IfcWall` or `IfcFooting` → `PhysicalLink`.

### Physical-axis nodes from IFC

For node matching against physical models, `bim_structural::ifc::physical_axes(&bytes, index, &frame)` returns an `AnalysisModel` with `kind = PhysicalAxes`: for each `IfcBeam`, `IfcColumn` and `IfcMember`, the 'Axis' representation's end points; failing that, the start and end of the body `IfcExtrudedAreaSolid` extrusion axis through the profile origin, flagged `approximate`. End points are welded within 0.1 mm into nodes; the member keeps the physical element id, the type name as section name, and the `CardinalPoint` from its material profile set usage, so the deviation report can account for it.

**Browser.** The reader is behind `cfg(not(wasm32))` with the rest of IFC. Whether `ifc-lite-core` and `-geometry` build for wasm32 inside this crate is an open question, checked by a non-blocking CI job. The `.samjson` path works in the browser from the start.

## Revit (`src/loaders/rvt.rs` + `rvt-read`)

Revit data maps in through three paths, in this order. Built-in parameter ids are mapped only from public Revit API `BuiltInParameter` names, as `schedules.rs` already does; never from Autodesk binaries or third-party parsers. Coordinates come from the reader's f64 values (feet × 0.3048), never from the f32 display mesh.

**R1: physical axes (no new class reverse engineering).** A `rvt_structural` module builds an `AnalysisModel` with `kind = PhysicalAxes`, `source.format = "rvt-derived"`:

- **Columns** (category −2001330; −2000100 architectural columns only on request): `(x, y)` from `Instance.origin`, z from `Document::level_elevation(base_level) + base_offset` and the top equivalent. Exact for vertical columns; slanted columns are skipped with a diagnostic.
- **Framing and braces** (−2001320): end points from the `FamilyInstance` location curve once its driver field is identified through the generic object layer (the analogue of `VWall::m_pCurveDriver → m_pCrv`). Until then, approximated from the instance frame and the stored bounding box along local x, as `mesh.rs` step 2280 does, with `NodeOrigin::DerivedFromPhysical { approximate: true }` and a larger default matching tolerance.
- **Section:** the type and family name as `Section.name`, with `Shape::I`/`Rect` from type parameters (b, h, tw, tf by parameter name) when the family stores them, else `Shape::Unknown`.
- **Links:** each axis keeps the physical element id, so a match row or clash selects the Revit element.

Revit physical members keep their saved-graphics mesh as clash solid ("as drawn", justification baked in); no sweep is built for them.

**R2: the analytical model.** One discovery run of `rvt classes` and `rvt objects --class Analytical…` in the `rvt-dll` repository, on an OpenAEC-owned or permitted structural file, lists the classes and fields: analytical member curve, analytical node position, releases, structural role, boundary conditions (category −2005301, filtered today at `mesh.rs:1303`) and the link to the physical element. From the 2023 format on, analytical members are separate elements that can be offset from the physical members; older files attach the analytical stick to the physical element, so both families of classes are mapped. The reader returns plain structs (`rvt_model::AnalyticalMember`, `AnalyticalNode`); `src/loaders/rvt.rs` maps them into `AnalysisModel` and the viewer bumps the rev in `Cargo.toml`. This waits on the licence decision for `rvt-dll` (`LicenseRef-Proprietary`, pinned to `open-research`).

**R3 (optional): live export.** OpenAEC's Revit plugin (public Revit API, reachable through its MCP server) can write `sam-json` v1 from an open document's analytical members. It needs no binary reverse engineering and serves as ground truth to validate R2.

## The FE-package integration point: what the clean-room DLL must deliver

The clean-room project reads the FE-package file and delivers a C-ABI library (the request calls it `rfem-dll`) plus its own spec. This section is the **complete contract** it must target; it is written without any knowledge of the source format. Nobody working in this repository opens FE-package files, SDKs or third-party parsers, and no FE-package project file enters this repository: fixtures are `.samjson` outputs produced by the clean room from its own models.

The same shape serves three purposes: the C-ABI payload, the `.samjson` file format (browser, tests, hand-over without a DLL), and the test fixtures.

### C ABI, version 1

```c
/* All functions extern "C", #[no_mangle], panic-safe (catch_unwind; never unwind across the boundary).
   Strings and JSON are UTF-8. A handle is used from one thread at a time; handles are independent. */

uint32_t    sam_abi_version(void);                      /* returns 1 */
const char* sam_extensions(void);                       /* static, nul-terminated, lower case, e.g. "ext1;ext2" */

typedef struct SamHandle SamHandle;
SamHandle*  sam_open_path(const uint8_t* path_utf8, size_t len, int32_t* status);
SamHandle*  sam_open_bytes(const uint8_t* data, size_t len, int32_t* status);  /* may return status 2 in v1 */

/* The model as sam-json v1 without result tables. Owned by the handle until sam_close. */
int32_t     sam_model_json(SamHandle* h, const uint8_t** out, size_t* out_len);

/* [{"result_set": <u64>, "tables": [<kind>, ...], "rows": n, "stations": m}, ...] as JSON.
   Owned by the handle until sam_close. */
int32_t     sam_result_index(SamHandle* h, const uint8_t** out, size_t* out_len);

/* One result block (layout below), produced lazily. Owned by the handle until sam_release or sam_close. */
int32_t     sam_result_block(SamHandle* h, uint64_t result_set, uint16_t kind,
                             const uint8_t** out, size_t* out_len);
void        sam_release(SamHandle* h, const uint8_t* block);

int32_t     sam_progress(SamHandle* h, float* fraction);  /* optional; 0..1 during long opens */
int32_t     sam_cancel(SamHandle* h);                     /* optional; the only call allowed from another thread */
const char* sam_last_error(SamHandle* h);                 /* nul-terminated, owned by the handle; NULL h = last open error on this thread */
void        sam_close(SamHandle* h);

/* Return codes: 0 ok, 1 not found / not present, 2 unsupported, 3 corrupt input, 4 cancelled, 5 internal. */
```

`src/loaders/fe_dll.rs` looks for the library next to the executable, then at `O3V_FE_DLL`, checks `sam_abi_version() == 1`, and registers through `loaders::add` for the extensions the library reports, so this repository hard-codes no FE-package extension. If the library is missing or has the wrong version, the extensions are not offered and the log says why; the app never crashes. The model JSON is read at open; result blocks are fetched on a worker the first time a result set is shown, through a `ResultSource` impl.

### `sam-json` v1

The document mirrors the types above. The JSON Schema is `crates/bim-structural/schema/sam-json-v1.schema.json`, generated from the serde types by a test that fails when the committed file is stale; the prose spec is `crates/bim-structural/docs/sam-json-v1.md`. Unknown fields are rejected (`deny_unknown_fields`).

```json
{
  "format": "sam-json", "version": 1,
  "source": { "application": "…", "file": "…",
              "global_transform": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1],
              "conventions": { "gravity_was": "-z", "mz_sign_was": "tension_minus_y",
                               "reactions_were": "on_structure", "result_signs": "verified",
                               "local_z_rule": "…" },
              "eccentricity_policy": "explicit" },
  "units": "SI",
  "origin": [0.0, 0.0, 0.0],
  "models": [ {
    "id": 1, "name": "Frame", "dimension": "3d",
    "materials": [ { "id": 1, "name": "S235", "E": 2.1e11, "G": 8.1e10, "nu": 0.3, "rho": 7850, "alpha_t": 1.2e-5 } ],
    "sections": [ { "id": 1, "name": "HEB 200", "material": 1,
                    "shape": { "I": { "h": 0.2, "b": 0.2, "tw": 0.009, "tf": 0.015, "r": 0.018, "slope": 0 } },
                    "props": { "A": 7.81e-3, "centroid": [0, 0], "shear_centre": [0, 0],
                               "Iy": 5.696e-5, "Iz": 2.003e-5, "Iyz": 0, "It": 5.93e-7 } } ],
    "nodes": [ { "id": 72057594037927937, "no": "1", "p": [0, 0, 0], "frame": null,
                 "support": { "dofs": ["fixed","fixed","fixed","free","free","free"], "frame": "global" } },
               { "id": 72057594037927938, "no": "2", "p": [0, 0, 4], "frame": null,
                 "support": { "named": "roller_x" } } ],
    "members": [ { "id": 144115188075855873, "no": "1", "kind": "beam",
                   "nodes": [72057594037927937, 72057594037927938], "geometry": "line",
                   "z": [1, 0, 0], "orientation": { "beta": { "angle": 0.0, "rule": "…" } },
                   "section": [1, null], "section_rotation": 0.0,
                   "insertion": { "cardinal_point": 8, "eccentricity": [0.0, 0.0],
                                  "end_offsets": { "start": [0,0,0], "end": [0,0,0], "frame": "local" } },
                   "releases": [ null, { "dofs": ["fixed","fixed","fixed","fixed","free","free"] } ] } ],
    "surfaces": [],
    "line_supports": [],
    "load_cases": [ { "id": 1, "no": "LC1", "name": "Self weight", "action_type": "permanent",
                      "action_source": "DEAD_LOAD_G", "self_weight": [0, 0, -1], "coefficient": 1.0 } ],
    "combinations": [ { "id": 1, "no": "CO1", "name": "ULS 1", "purpose": "uls",
                        "terms": [ [ { "case": 1 }, 1.35 ] ] } ],
    "actions": [ { "id": 1, "load": { "case": 1 }, "target": { "member": 144115188075855873 },
                   "frame": "global",
                   "kind": { "member_distributed": { "dir": [0,0,-1], "moment": false,
                             "dist": { "uniform": 5000.0 } } } } ],
    "results": [ { "id": 1, "no": "CO1", "name": "ULS 1", "for": { "combination": 1 },
                   "theory": "first_order", "linear": true,
                   "tables": ["member_forces", "reactions"],
                   "inline": null } ]
  } ]
}
```

Field rules (the conformance check enforces each):

- **Units and frame.** SI only; world metres, right-handed, Z up. A source with gravity +Z is rotated 180° about X, recorded in `global_transform` and `gravity_was`. `origin` is the anchor the viewer subtracts for display (the producer may choose any point near the model; matching uses world coordinates).
- **Ids.** `sam_id(kind, number)` from the `IdKind` table (node 1, member 2, surface 3, section 4, material 5, line support 6, load case 7, combination 8, action 9, result set 10, opening 11), where `number` is the source number. Ids are stable for the same file across opens. Sections, materials, cases, combinations, actions and result sets may use small plain numbers inside the JSON (as in the example) because they are referenced only within the document; nodes, members, surfaces and line supports must use the shifted form. `no` keeps the source numbering as text.
- **DOFs.** Each DOF is `"fixed"`, `"free"`, `{"spring": k}` (N/m or N·m/rad; per length for line supports), or `{"tension_only": k}`, `{"compression_only": k}`, `{"failure": {"k": k, "limit": f}}`, `{"slippage": {"k": k, "gap": g}}`, `{"diagram": [[d, f], ...]}`. For supports `fixed` means restrained; for member ends `fixed` means connected and `free` released. Instead of `dofs`, a support or release may give `{"named": "<name>"}` from the preset tables above when the source only has a type; the producer must not invent DOFs it does not know. Support and release `frame` is `"global"`, `"node"` (with the node's `frame` 3×3 given) or `"member"`; releases are always member-local.
- **Members.** `nodes` lists start, intermediate (ordered along the member) and end. `geometry` is `"line"`, `{"arc": {"mid": [x,y,z]}}` or `{"polyline": [[x,y,z], ...]}`. **`z` is always given**: a unit vector perpendicular to the member axis, already resolved with the source's own default rule and its rotation angle; `orientation` is provenance only (`{"beta": {"angle": rad, "rule": "text"}}`, `{"ref_node": id}`, `{"ref_vector": [..]}`, `"default"`). This repository never interprets a vendor beta rule. `kind` is one of `beam`, `truss`, `cable`, `tension_only`, `compression_only`, `rigid`, `spring`.
- **Insertion.** `cardinal_point` uses the IFC4 numbering 1-19 (0 = none). `eccentricity` is the member-wide profile offset (ey, ez) in local axes applied after the cardinal point (the profile centroid moves by +e). `end_offsets` are rigid offsets from node to member end with their frame. The producer splits every source offset into exactly one of these fields and states the mapping in its spec; with `eccentricity_policy: "explicit"` nothing is inferred on this side. If the source places sections by a reference point that is not one of the 19 cardinal points, the producer gives `cardinal_point: 0` and puts the full offset of the profile's bounding-box centre into `eccentricity`.
- **Sections.** `shape` is one of `I`, `AsymI`, `U`, `L`, `T`, `C`, `Z`, `Rect`, `Circle`, `Chs`, `Rhs` with the parameter names of `Shape` in metres, or `{"polygon": {"outer": [[u,v],...], "holes": [[[u,v],...]]}}`, or `{"composite": [...]}`, or `"unknown"` with only a `name`. The profile origin is the bounding-box centre. `props` are given when the source has them (they win for labels; geometry uses the outline); `shear_centre` is needed for cardinal points 15-19 on asymmetric shapes.
- **Loads.** `frame` is `"global"`, `"local"` or `"global_projected"` (projected only with global directions). Distributions: `{"uniform": q}`, `{"linear": {"q1", "q2"}}`, `{"trapezoidal": {"a", "b", "q1", "q2"}}` (absolute metres), `{"polygonal": [[x, q], ...]}`, `{"parabolic": peak}`, `{"sine": peak}`. Point actions on members give absolute `x`. Combinations list their terms with factors; the producer never computes combination results the file does not hold.
- **Results.** Internal forces in the canonical sign convention (or converted, with `mz_sign_was` saying from what); reactions as the force on the structure (`reactions_were` saying what the source used); `result_signs` is `"verified"` when the producer has confirmed its mapping, otherwise `"assumed_canonical"`. Stations in absolute metres along the member, duplicated at jumps. Small models may carry blocks inline as base64 under `inline: { "<kind>": "…" }`; otherwise they come from `sam_result_block` or a sidecar file `<name>.<set>.samr`.
- **Diagnostics.** `"diagnostics": [{"object": id, "code": "…", "message": "…"}]` for everything the producer could not map.

### Result block (`sam_result_block`, or a sidecar `.samr`)

Little endian, no padding:

```
magic     "SAMR"            4 bytes
version   u16 = 1
kind      u16               1 member forces, 2 member deformations, 3 node displacements, 4 reactions, 5 envelope
set       u64               result set id
rows      u32               members or nodes
stations  u32               total stations (member kinds); 0 for node kinds
ids       u64[rows]
offsets   u32[rows + 1]     member kinds only
x         f64[stations]     member kinds only
values    f64[6][n]         component-major; n = stations (member kinds) or rows (node kinds)
                            member forces: N Vy Vz Mt My Mz; deformations: ux uy uz rx ry rz (local)
                            node displacements: ux uy uz rx ry rz (global); reactions: Fx Fy Fz Mx My Mz (support frame)
envelope  kind 5: as kind 1, then u64 governing_max[6][stations], governing_min[6][stations] (result set ids),
          values holding max then min (f64[12][stations])
```

This maps one-to-one onto `MemberTable` / `NodeTable` without parsing.

### Conformance and the mock

- `cargo run -p bim-structural --bin sam-check -- model.samjson [blocks...]` validates a document and its blocks: schema, id scheme, unit-length `z` perpendicular to every member (within 1e-9), node references, cardinal points in 0-19, ascending stations with matched jump pairs, `sam_id` kinds, no NaN or infinity, and every rule above. The clean room runs it on its output; this repository runs it in CI on the fixtures the clean room hands over (`crates/bim-structural/tests/fixtures/conformance/*.samjson`).
- `crates/sam-abi-mock` builds a `cdylib` implementing the whole ABI from a `.samjson` (with sidecar blocks), so `fe_dll.rs` is tested before the real library exists, and the clean room can compare its library's behaviour with the mock's.
- CI checks that no FE-package project files enter the repository (a file-type allow-list under `tests/`).

## Rendering

### Budget and principle

The 120 fps criterion (p95 ≤ 8.33 ms on the reference PC; no regression against the baseline on integrated graphics) applies with every structural layer switched on. A few instanced draws cost microseconds on the GPU; the risk is CPU work. So: all symbols are GPU instances built once per model or once per selected case; every interactive control (scale sliders, component chips, case selector) writes a uniform or swaps a buffer range; only text uses canvas, bounded.

### Three pipelines (`src/viewport/structural.rs`, `structural.wgsl`)

All three bind `model_group` at `@group(0)` (model matrix, section clip, units texture), so the model matrix, section box and plane (`clipped()`), exploded offsets and #91 flags apply, and they draw in the existing MSAA pass. Structural state is a separate uniform buffer at `@group(1)`, so the existing `Uniforms` and the hot scene pipelines are not changed (only the tint palette binding in its own PR touches them):

```rust
#[repr(C)]
struct StructUniforms {
    viewport: [f32; 4],      // width_px, height_px, 1/width, 1/height
    sizes: [f32; 4],         // line_px, glyph_px, hinge_setback_px, selected_line_px
    scales: [f32; 4],        // diagram (m per SI unit), load (m per N or N/m), deformation factor, proj11
    lod: [f32; 4],           // min_glyph_px, arrow_spacing_px, node_cluster_px, diagram_side (+1/-1)
    colors: [[f32; 4]; 8],   // axis, node, support, hinge, load, diagram+, diagram-, deformed
}
```

1. **`seg` (wide segments).** An instanced 4-vertex triangle strip per segment, expanded in clip space to a quad `sizes.x` pixels wide (`sizes.w` when the unit is selected): the `vs_point` technique stretched to a segment. Instance (≤ 8 attributes): `a: Float32x3, a_off: Float32x3, b: Float32x3, b_off: Float32x3, color: Unorm8x4, unit: Uint32, style: Uint32`. The final endpoint is `a + a_off · scales[style.scale]`, where `style` selects no scale, diagram, load or deformation scale, and holds a dashed bit and a width multiplier. Both ends go through `place()` (unit offset, `NOWHERE` when hidden); a segment crossing the near plane is clipped against `w = ε` before expansion. Depth is written, so axes hide behind solids. One primitive draws member axes, rigid end offsets (thick), diagram and load outlines, the deformed shape, line-support base lines, deviation vectors and clash box edges.
2. **`glyph` (3D symbols at screen size).** Small lit 3D meshes from one shared vertex and index buffer. Instance (≤ 8 attributes): `pos: Float32x3, rot: Float32x4 (quaternion), color: Unorm8x4, unit: Uint32, size: Float32x2 (px, world cap), param: Float32x2 (shaft length or setback), kind_lod: Uint32`. The vertex shader computes world-per-pixel at the instance from its clip `w`: `wpp = 2 · clip.w / (viewport.y · proj11)` (perspective and orthographic), scales the unit mesh by `min(size.x · wpp, size.y)`, and lights it with a headlight. `size.y` is a world-space cap per instance (0.4 × the shortest member at that node, computed at build), so symbols never swallow short members when zoomed out. Instances are sorted by glyph kind, one indexed draw per kind (about 15 draws). **WebGL2:** no `base_vertex` and no non-zero `first_instance`, so each kind has its own vertex and index buffer slice and its instance range is bound with `set_vertex_buffer` at an offset, never by `first_instance`.
3. **`fill` (diagram and load areas).** World-space triangles, unlit, alpha-blended (alpha 0.55), depth test on, depth write off, culling off. Vertex: `base: Float32x3, off: Float32x3, color: Unorm8x4, unit: Uint32, scale_sel: Uint32`, position `base + off · scales[scale_sel]`.

Draw order in the existing pass: after lit layers and point clouds, before the cut fill: glyphs and segments; after glass: fills; then datum and ground grids. An *Axes on top* toggle draws segments and glyphs a second time with depth compare `Always` at 40 % alpha, so axes inside physical solids stay readable.

### Glyph library (`src/viewport/structural/glyphs.rs`)

Built once at pipeline creation from procedural meshes (a few hundred triangles each), in a unit frame where +Z is the support "down" or arrow direction. Colour per translation axis x red, y green, z blue.

| Glyph | Shape | Orientation |
|---|---|---|
| Fixed support | block with a base plate | support frame |
| Pinned support | pyramid on a plate | support frame |
| Roller support | pyramid on two cylinders, rollers along the free direction | support frame + free axis |
| Sliding support | block on rollers | as roller |
| Spring | coil (translational) or spiral disc (rotational) | DOF axis |
| Custom support | a cone per restrained translation, a ring per restrained rotation, a coil per spring | support frame |
| Line support | hatch comb repeated along the carrier (instances at binary-level spacing), plus a `seg` base line | carrier and support frame |
| Node | small cube | none |
| Hinge (moment) | open ring, camera-facing | set back `hinge_setback_px` along the member |
| Hinge (one axis) | ring in the rotation plane | member frame |
| Torsion release | ring around the member axis | member frame |
| Axial release | two parallel plates | member frame |
| Spring / nonlinear release | half-filled ring | member frame |
| Arrow | shaft (world length `param.x · load scale`) + head (px) | load direction |
| Moment arrow | double head along the moment vector | moment vector |
| Clash / match marker | sphere | none |

2D models use the textbook 2D symbols (triangle, triangle on circles, hatched base) as the same meshes flattened into the model plane.

### Level of detail, in the vertex shader

- **Glyph cut-off:** when the drawn size falls below `lod.x` pixels (default 3), the instance goes to `NOWHERE`.
- **Node clustering:** node and hinge glyphs carry the shortest adjacent member length; when it projects to fewer than `lod.z` pixels (default 6) they are dropped. Supports are never dropped, only shrunk by the world cap.
- **Arrows by level:** a distributed load places arrows at binary subdivisions of its length; each instance carries its level k (0 = both ends, 1 = middle, 2 = quarters, ...) and is drawn when `length / 2^k` projects to at least `lod.y` pixels (default 18). Zooming in adds arrows; zooming out leaves the ends and the outline.
- **Segments** never drop out.

Zooming therefore costs no CPU.

### Loads

- A distributed load is a fill trapezoid between the axis and `axis − dir · q(x) · s` (polygonal and parabolic and sine sampled at 16 points), an outline `seg`, and arrows at binary levels with heads touching the axis. Local loads are drawn perpendicular to the member in their local direction; global loads in their direction; projected loads over the projected length, with "proj." in the label and the true-length intensity in properties.
- Point force: an arrow whose head touches the point. Moment: a double arrowhead along the vector. Temperature: the member tinted and labelled `ΔT`. Prescribed displacement: a double-headed arrow labelled `u =`.
- **Switching:** load geometry is small, so at load all cases are built into one `fills` buffer and one `seg`/`glyph` buffer with per-case draw ranges; switching a case changes a range. Above 32 MB of load geometry the build falls back to per-case on demand, like results.

### Diagrams and results

- A diagram vertex stores its point on the axis (`base`) and `off = side × value`, `value` the station value in SI and `side` the unit vector of the plotted side: `My → +ẑ`, `Mz → −ŷ` (tension side under the canonical convention, whichever way local z points). N, Vy, Vz and Mt are drawn along a chosen local axis (default z) and coloured by sign. `lod.w` flips the moment side for users who want positive moments on the compression side (a uniform, no rebuild).
- Polygons are split at sign changes (interpolated zero crossing) so fills never self-overlap; duplicate stations draw jumps vertically.
- **Auto scale** sets `scales.x` so that the largest |value| of the visible elements plots at 8 % of the model's bounding-box diagonal; the slider multiplies that (0.1× to 10×, logarithmic). Changing it is a uniform write.
- Reactions are arrows at supports. The deformed shape is `seg` instances from displaced stations, with cubic Hermite interpolation from end displacements and rotations when only end values exist; the factor is `scales.z`.
- **Switching:** selecting a result set and component builds its instance and fill arrays on a worker (desktop) or synchronously in `update` (wasm; about 3 ms for 10k members × 20 stations), writes them into a growable `COPY_DST` "active structural" buffer pair with `queue.write_buffer`, and keeps the last four selections in a CPU LRU. Until the new revision is ready the previous one is drawn; a camera frame never waits.
- A **legend** (canvas, bottom left) shows component, unit, min and max with member and station; clicking min or max selects and frames it.
- While `result_signs = AssumedCanonical`, the legend shows "sign convention not verified for this source".

### Labels (`src/structural/labels.rs`)

Text stays in an iced canvas overlay pushed after `measurements` in the stack at `src/app.rs:2149`, bounded:

- While the camera moves: only labels of the selection and the hover, at most 32.
- When the camera has been still for 120 ms: one label pass projects candidates, keeps those on screen, sorts by priority (selected, hovered, extremes of the active diagram, reactions, member numbers, node numbers), places one label per cell of a 64 × 20 px screen grid, and caps at 256. The result is drawn from a `canvas::Cache` keyed by (camera revision, display revision).
- Modes: *Off*, *Selection*, *Extremes* (default when a result is shown), *Numbers*, *All (capped)*. Values in UI units (kN, kN/m, kNm, mm).

A GPU glyph atlas for text is left for later.

### Picking, hover, box select, section, explode

- **Pick** (`src/structural/pick.rs`): `App::pick` calls it before `pick_triangle`. It projects the visible nodes, glyph centres and member segments (clipped to the frustum) and takes the nearest node within 8 px, else glyph within 8 px, else member within 6 px. If a triangle hit is closer in depth by more than 1 % of the view distance and *Axes on top* is off, the triangle wins. The result is `Pickable::Element(id)`. About 0.5 ms for 10⁵ objects, on click only.
- **Hover** uses a screen-space uniform grid built when the camera has been still for 120 ms and dropped when it moves; hover is skipped while the camera moves.
- **Box select, hide, isolate, undo:** through the structural units in `ExplodeIndex`, #91's `units_in_box` and history work unchanged.
- **Section box and plane:** the structural fragment shaders call `clipped()`; member solids are a layer, so the cut fill works.

### The Structural tab (`src/structural/panel.rs`)

Commands go into the command list in `src/shell.rs` with Dutch and English labels.

- **Show:** axes, nodes, supports, hinges, line supports, *Members: axes / solids / both*, *Ghost physical* (other layers as glass), *Axes on top*, labels mode.
- **Loads:** load case and combination drop-down, load scale, global/local arrows.
- **Results:** result set drop-down (only sets in the file), component chips `N Vy Vz Mt My Mz | u`, diagram scale with *Auto*, tension/compression side, deformation scale.
- **Model notes:** the diagnostics, grouped by code.
- **Properties panel** (`App::properties_body`) gains a structural section: for a member the section, length, local axes, cardinal point, offsets with their sources (`OffsetNote`), releases as codes at both ends, and the extremes of the active result with their stations; for a node the f64 coordinates in mm, the support code and name, and the reaction of the active result; for a load its case, frame and values.

## Profile solids with cardinal point and y/z eccentricity (`bim_structural::solid`)

### Placement formula

For a straight member with start node `P0`, end node `P1`:

1. **Ends:** `P0' = P0 + off₀`, `P1' = P1 + off₁` (rigid end offsets; member-local offsets rotated by the member frame built from `P0 → P1`).
2. **Frame:** `x = normalize(P1' − P0')`, `z = normalize(z_ref − (z_ref·x) x)`, `y = z × x`, `L' = |P1' − P0'|`.
3. **Outline** in profile `(u, v)`, origin at the bounding-box centre, then rotated by `section_rotation`.
4. **Insertion** (`insertion::resolve(&Insertion, &SectionProps, policy) -> Resolved { rotation: f64, translate: DVec2, notes: Vec<OffsetNote> }`), the single precedence function:
   - With `cardinal_point ≠ 0` and policy `CardinalCancelsPosition`: apply the profile `Position` (rotation and translation) to the outline, then compute the anchor `a` on that transformed outline, so the `Position` translation cancels out and only its rotation remains. If the cancelled translation was non-zero, note `double-eccentricity-suppressed` with its value.
   - With `cardinal_point = 0`: keep `Position` as given; `a = 0`.
   - With `Additive`: anchor on the untransformed outline, then add the `Position` translation.
   - With `Explicit`: `Position` must be identity (else a diagnostic) and the anchor is the cardinal point on the outline.
   - `profile_offsets` (start and end, IFC `IfcMaterialProfileWithOffsets`) add to the eccentricity at each end (a skewed sweep when they differ), with a note.
   - `eccentricity (ey, ez)` always applies: the profile moves by +e.
   - A per-model **Ignore cardinal point** setting (for exporters that encode the same offset twice) forces `a = 0` and notes it.
5. **Sweep:** a profile point `(u, v)` lands at

   `P(u, v, t) = P0' + y · (u − a_u + e_y) + z · (v − a_v + e_z) + x · t`, `t ∈ [0, L']`,

   which is the affine map with columns `[y, z, x, P0' + y·(e_y − a_u) + z·(e_z − a_v)]`. The extrusion axis is member x: the IFC axis-naming swap.

Every solid stores its provenance (`OffsetNote { kind: CardinalPoint(u8) | ProfilePosition | ProfileOffsets | Eccentricity | EndOffset(End) | IgnoredCardinal, value: DVec3, source: String }`), shown in properties and returned by `get_structural_element`.

### Other members

- **Tapered** (`section.1` set): loft between start and end outlines resampled to equal vertex counts per loop (by arc length), anchors computed per end.
- **Curved** (`Arc`, `Polyline`): rings at each sample with the member frame (z_ref projected perpendicular to the tangent; rotation-minimising when there is none) joined into a tube; polylines mitre at vertices.
- **Caps:** `earcutr` (polygon with holes) in the crate's own code on every target. `ifc-lite-geometry` is not used for sweeping, so `.samjson` solids work in the browser.
- **Output:** `SolidMesh { positions: Vec<DVec3>, indices: Vec<u32>, closed: bool, placeholder: bool }`, outward normals, closedness checked with a port of the viewer's `encloses_volume` logic.
- **Joints:** `JointSetback::{None, HalfDepth}`. *HalfDepth* shortens a member end at a node by half the largest section depth of the other members there. Default *None* (true analytical geometry); the clash engine excludes shared-node pairs instead.
- **Surface members:** the reference polygon offset by `offset_z ± t/2` along the normal, with openings and side walls. `Thickness::Unknown` draws the face only and is excluded from hard clash.
- **Unknown sections** (`Shape::Unknown`): a 50 × 50 mm placeholder box, `placeholder: true`, drawn hatched (glass), excluded from hard clash by default.
- **Refusals** (zero length, z parallel to x, tessellation failure) are listed in *Model notes*; the member keeps its axis. Refusals are never silent.

### Display

One `Layer` named `Analysis members`, coloured by material class (steel, concrete, timber, other), element ids = member and surface ids, positions f32 relative to the part's origin. It is built on the load worker (threads on desktop, serial on wasm) before the model is published; *Members: axes / solids / both* is ordinary layer visibility. About 100-300 triangles per member: 10⁵ members in under 2 s on one desktop worker.

## Node matching (`bim_structural::matching`, UI in `src/structural/matching.rs`)

### Inputs and options

```rust
pub struct NodeSet<'a> {
    pub model: u32,                       // ElementKey.model
    pub ids: &'a [Id],
    pub pos: &'a [DVec3],                 // world metres
    pub axes: &'a [(Id, DVec3, DVec3)],   // member axes of this set, for project-to-axes
    pub approximate: &'a [bool],
}

pub enum Tolerance { Sphere(f64), Cylinder { horizontal: f64, vertical: f64 } }
pub enum Alignment { None, Translation(DVec3), Pairs(Vec<(Id, Id)>), Auto }

pub struct MatchOptions {
    pub tolerance: Tolerance,             // metres, inclusive
    pub alignment: Alignment,
    pub project_to_axes: bool,            // also test unmatched A nodes against B member axes
    pub within_model: f64,                // coincident-node check; default = horizontal tolerance
    pub members: bool,                    // also match members and compare attributes
}
```

**Node sets** offered in the UI: *analysis nodes* of any loaded structural part; *physical axis ends* from IFC (`ifc::physical_axes`) or Revit (R1); *all structural nodes*. Defaults: 10 mm for analysis vs analysis; 100 mm with *project to axes* on when one side is physical (physical axes sit offset by cardinal point and eccentricity).

### Algorithm

1. **Frame.** Both sets in f64 world metres, made relative to the mean of A to keep precision at survey coordinates.
2. **Alignment.** `None` trusts world coordinates. `Translation` is user input. `Pairs` fits a translation from one picked pair or a rigid transform by Kabsch/SVD (3×3 Jacobi SVD in `glam`, about 40 lines) from three or more. `Auto` takes the component-wise median of nearest-neighbour vectors (sampled up to 10⁴ nodes) as an initial translation, then up to ten rounds of Kabsch on mutual nearest pairs within 5 × tolerance until the RMS change is below 1e-6 m. The report includes the transform and RMS; *Apply to display* moves model B's display transform (a model matrix, no re-upload).
3. **Candidate search.** Uniform hash grid over B with cell size = the largest tolerance component, keys `floor(p / cell)` as `(i64, i64, i64)` in an `FxHashMap`; each A node probes the 27 neighbour cells. Comparisons are inclusive (≤). `Cylinder` tests `hypot(dx, dy) ≤ h && |dz| ≤ v`. O(n) overall.
4. **Classification.** Union-find over the bipartite A–B pairs within tolerance:
   - 1:1 components are **matched**: *exact* when |d| ≤ min(1 mm, tol/2), *within tolerance* above that.
   - 1:n, n:1 and n:m components are **ambiguous**, with the best mutual-nearest pairing proposed.
   - Singletons are **unmatched A** or **unmatched B**; with *project to axes*, an unmatched A node within tolerance of a B member axis is **on axis, no node** (with the member and the station).
   - The same grid run on each set alone gives **coincident** pairs inside one model (a modelling defect).
5. **Curve.** `nn_distance_a` (nearest B distance per A node after alignment) is computed once per alignment, independent of the tolerance, so the *matched fraction vs tolerance* curve and the slider need no re-run until the alignment changes.
6. **Members** (option). A member of A whose end nodes are matched corresponds to a B member with the same ends in either order, or to a chain of collinear B members through matched intermediate nodes (*split*). Attribute deltas: section name and shape, angle between z vectors (modulo 180° for doubly symmetric sections), releases and support DOFs per DOF in a common frame, links.

```rust
pub struct MatchReport {
    pub transform: DAffine3, pub rms: f64,
    pub matched: Vec<Pair>,                        // Pair { a, b, delta: DVec3, class: Exact | Within }
    pub ambiguous: Vec<Cluster>,                   // ids on both sides + proposed pairing
    pub unmatched_a: Vec<Id>, pub unmatched_b: Vec<Id>,
    pub on_axis: Vec<(Id, Id /*B member*/, f64 /*station*/, f64 /*distance*/)>,
    pub coincident_a: Vec<(Id, Id, f64)>, pub coincident_b: Vec<(Id, Id, f64)>,
    pub nn_distance_a: Vec<f64>,
    pub members: Option<MemberMatchReport>,
}
```

### UI

- Panel *Compare nodes*: set A and set B pickers, tolerance slider (logarithmic, 0.1 mm to 1 m) with a typed field in mm, *Sphere / Horizontal + vertical*, alignment (with *pick pair* on the canvas), *project to axes*, *members*.
- The matched-fraction curve (small canvas chart) under the slider with a marker at the current tolerance, so the engineer sees where the knee is.
- Result chips with counts and palette colours: exact green, within tolerance yellow-green, deviation above tol/2 amber, ambiguous purple, unmatched A red, unmatched B magenta, on axis cyan, coincident blue. Clicking a chip isolates those nodes and their members (#91 isolate, undoable); a second click restores.
- Colours are tint bits; moving the slider re-classifies from `nn_distance` and the cached grid (debounced 50 ms) and rewrites only the units texture. Nodes and the physical elements they derive from are tinted alike.
- Deviation vectors as `seg` instances with an arrow glyph from A to B, exaggeration ×1 to ×100 (a uniform), in a small `COPY_DST` overlay buffer.
- Table: A number, B number, dx, dy, dz, |d|, class; sortable; clicking a row selects both nodes through `Selection` and frames them.
- Export: CSV and JSON (both products); BCF topics per unmatched or ambiguous cluster (validator, through the extended `bim-bcf`).

The node-matching tolerance and the clash tolerance are separate settings.

## Clash engine (`crates/bim-clash`, shared by viewer and validator)

### API

```rust
pub struct ItemKey { pub model: u32, pub source: u64 }     // = ElementKey (#91)

pub struct ClashItem<'a> {
    pub key: ItemKey,
    pub global_id: Option<&'a str>,
    pub class: &'a str,                                    // IfcType, category or structural kind
    pub origin: DVec3,                                     // world = origin + position
    pub positions: Cow<'a, [[f32; 3]]>,                    // display layers: f32 relative to origin
    pub positions_f64: Option<&'a [DVec3]>,                // structural solids: the f64 evaluator output
    pub indices: Cow<'a, [u32]>,
    pub closed: bool,
    pub placeholder: bool,
    pub nodes: SmallVec<[u64; 4]>,                         // analytical end nodes (joint rule)
    pub joint_radius: f64,                                 // half the section diagonal
    pub links: SmallVec<[ItemKey; 2]>,                     // physical counterpart(s)
    pub storey: Option<&'a str>,
}

pub struct ClashTest {
    pub id: String, pub name: String,
    pub a: Selector, pub b: Selector,                      // by model, layer, class, property value, structural kind
    pub mode: Mode,                                        // Hard { tolerance } | Clearance { distance }
    pub same_model: bool,
    pub joints: JointRule,                                 // Exclude (default) | Report
    pub linked: LinkRule,                                  // Deviation (default) | Clash | Ignore
    pub placeholders: bool,                                // include placeholder solids (default false)
}

pub enum ClashKind { Hard { depth: f64 }, Contained, Touch, Clearance { distance: f64 }, Joint }

pub struct Clash {
    pub id: Uuid,                                          // v5(namespace, test id + sorted GlobalIds or keys)
    pub test: String, pub a: ItemKey, pub b: ItemKey,
    pub kind: ClashKind, pub point: DVec3, pub bbox: [DVec3; 2],
    pub storey: Option<String>,
    pub volume: Option<f64>,                               // on demand, feature "exact"
}

pub struct Deviation {                                     // analytical member vs its linked physical element
    pub analytical: ItemKey, pub physical: ItemKey,
    pub axis_offset: f64, pub angle: f64, pub section_mismatch: Option<(String, String)>,
}

pub fn run(items: &[ClashItem], tests: &[ClashTest], cancel: &AtomicBool,
           progress: &dyn Fn(f32)) -> (Vec<Clash>, Vec<Deviation>);
pub fn overlap_volume(a: &ClashItem, b: &ClashItem) -> Option<f64>;  // feature "exact"
```

### Algorithm

1. **Items** come from two feeds, both the drawn geometry: display layers through `Model::element_ranges` (every format, `origins[model] + f32`), and structural solids from the same `SolidMesh` that fed the `Analysis members` layer (f64 directly).
2. **Broad phase.** Per item an f64 AABB padded by the tolerance or clearance; a binned-SAH BVH over set B; one query per A item. Pairs are filtered before the narrow phase: same element (one key across layers), excluded classes, same model unless allowed, placeholders unless allowed, **members sharing a node or a rigid link** (`JointRule::Exclude`), and **an analytical member and its linked physical element** (to the deviation report under `LinkRule::Deviation`; links from `IfcRelAssignsToProduct`, Revit associations, or a node-matching run).
3. **Narrow phase.** A triangle BVH per item (median split, 8 triangles per leaf, f64 boxes), built lazily and cached for the run; dual BVH traversal; triangle-triangle intersection with exact predicates (`ifc_lite_geometry::kernel::tritri` under feature `exact`, the desktop default) or an f64 Möller interval test with a coplanar branch and an epsilon of 1e-9 × scale (wasm, no feature). Up to 64 intersection segments per pair.
4. **Classification.**
   - No segments and both meshes closed: a point-in-mesh test (ray parity, three fixed directions, majority) gives `Contained`, else nothing.
   - All segments from coplanar or near-coplanar triangle pairs within the tolerance: `Touch` (hidden by default).
   - Both are analytical members with a shared node n and every segment within `joint_radius_a + joint_radius_b` of n: `Joint` (only when `JointRule::Report`).
   - Otherwise `Hard`, with a depth estimate: the largest distance to the other surface among vertices of one inside the other (triangle-BVH distance queries), else the half length of the longest intersection segment. Hard below the tolerance becomes `Touch`.
   - `Clearance`: minimum triangle-triangle distance below the distance.
5. **Result.** `point` is the mean of the segment midpoints (or the contained item's centre); `bbox` the box of the segments. `volume` via `intersection_solid` only when a clash is opened.
6. **Determinism.** Pairs are sorted by key; ids are UUIDv5 over the test id and the sorted GlobalIds (keys with the model's file name when GlobalIds are missing), so a re-run keeps BCF topics.
7. **Threads.** `rayon` over candidate pairs under feature `parallel` on the desktop; single-threaded otherwise. Always a background job (the `job()` and generation-guard pattern of `src/validation.rs`, cancellable), never in `update` or on the frame path.

**Deviation report.** For each linked or matched pair: axis offset (distance between the analytical axis and the principal axis of the physical solid, after the physical element's own cardinal point when known), angle between them, and section name mismatch. This is the "is the analysis model where the physical model is" check.

### Integration

- **`src/clash.rs`** (lib, both products): builds `ClashItem`s from the loaded models (one per id, gathered across layers through `element_ranges`; `nodes`, `joint_radius` and `links` from the `StructuralPart`), runs jobs, and handles the MCP tools. Results are shown with #91 flags (select and isolate the pair, undoable), tint bits (A red, B amber), a marker glyph at `point` and an optional section box around `bbox` + 0.5 m. The viewer product gets the engine through MCP first.
- **Open BIM Validator** (after #79 is rebased on `main` with #91): a *Clash* tab in `src/validation.rs` with a tests editor (A and B selectors, mode and tolerance in mm, joint and link rules), saved as `clash-rules.json` next to the IDS files; run with progress and cancel; results grouped by test, storey and element pair, with status (New, Active, Resolved, Approved); clicking a clash selects, isolates, tints and frames it.
- **BCF** (`crates/bim-bcf`): `NewTopic` gains `viewpoint: Option<Viewpoint { camera: Camera, selection: Vec<String>, coloring: Vec<(String /*ARGB*/, Vec<String>)>, clipping: Vec<ClippingPlane> }>`, written to `viewpoint.bcfv`; the clash point goes into the topic description. Elements without a GlobalId get a deterministic IFC GUID from the same UUIDv5. BCF import that applies cameras and statuses, and offscreen snapshots, come later.
- **Headless:** `open-bim-validator --clash clash-rules.json a.ifc b.samjson --bcf out.bcfzip --json out.json --csv out.csv` in `src/validation_cli.rs`.
- **`select_finding`** is rewritten on #91 `Selection`/`ElementKey` with a GlobalId → key index built once per load, replacing the linear props-JSON scan.

## Validator model checks

Rules in `bim-validator-core` with `RuleSource::ModelChecks`, ids `SAM-01..`, calling `bim_structural::checks::run(&AnalysisModel)`. Geometric and topological consistency only, never mechanics:

| Id | Check |
|---|---|
| SAM-01 | coincident nodes within one model (closer than 1 mm) |
| SAM-02 | free node (no member, no surface, no support) |
| SAM-03 | zero-length member |
| SAM-04 | member without section, or with an unknown section |
| SAM-05 | local z parallel to the member axis |
| SAM-06 | model without supports |
| SAM-07 | member released in all rotations at both ends (warning) |
| SAM-08 | part of the model without a connection path to a support (graph connectivity) |
| SAM-09 | action or result referring to a missing item |
| SAM-10 | result set for a load case or combination not in the model |
| SAM-11 | negative spring stiffness |
| SAM-12 | redundant cardinal point and profile position (the suppressed double eccentricity) |

## MCP tools

Each tool gets one entry in `tools()` (`src/bin/open-3d-viewer-mcp.rs:66-184`) and one arm in `App::command` (`src/remote.rs:303-347`).

| Tool | Read-only | Arguments | Returns |
|---|---|---|---|
| `list_structural` | yes | | per part: model, name, kind, source, counts of nodes, members, surfaces, line supports, sections, cases, combinations, result sets; diagnostics count |
| `get_structural_issues` | yes | `model` | diagnostics |
| `get_structural_element` | yes | `id` | node, member, surface, support or load with coordinates (f64, metres), frame, section, insertion with offset notes, releases, support codes and names, active result extremes |
| `find_structural` | yes | `kind`, filters (support preset, section name, member kind, number range), `offset`, `limit` | ids |
| `list_load_cases` | yes | `model` | cases and combinations with factors |
| `list_result_sets` | yes | `model` | sets with theory, linearity, available tables, sign status |
| `get_member_results` | yes | `id`, `result_set`, `component?` | stations and values in SI and in display units |
| `get_node_results` | yes | `id`, `result_set` | displacement and reaction |
| `set_structural_display` | no | show flags, `members: axes\|solids\|both`, `load_case`, `result_set`, `component`, scales or `auto`, `side`, `labels` | applied state |
| `match_nodes` | no (changes tints) | `set_a`, `set_b` (model + node set), `tolerance_mm`, `mode`, `vertical_mm`, `alignment`, `pairs`, `project_to_axes`, `members` | counts per class, transform, RMS, first 100 deviations by size |
| `get_node_matches` | yes | `class`, `offset`, `limit` | rows (A, B, dx, dy, dz, \|d\|, class) |
| `clear_node_matches` | no | | |
| `run_clash` | no (changes tints) | `tests` (inline JSON or a path) | run id, counts per test and kind |
| `list_clashes` | yes | `test`, `kind`, `status`, `offset`, `limit` | clashes with keys, GlobalIds, kind, point |
| `show_clash` | no | `id` | selects, isolates, tints, frames the pair |
| `get_clash` | yes | `id`, `volume?` | the full clash, with overlap volume on request |
| `get_deviations` | yes | `offset`, `limit` | analytical vs physical deviations |
| `set_clash_status` | no (validator) | `id`, `status` | |
| `export_bcf` | no (validator) | `path`, `source: clashes \| findings \| node_matches` | written path |

`get_element`, `find_elements`, `select`, `hide_elements`, `isolate_elements` and `zoom_to` work on structural ids through `StructuralIndex` and `Model::element_bounds`.

## Performance

- **Bench scenario:** `--bench structural:N --throughput --frames 300` (`src/bench.rs`): a 3D grid frame of N members with their nodes, supports on every ground node, hinges on 10 % of the ends, one load case, an My diagram with 11 stations per member, labels in *Numbers* mode. Run alone and together with `synthetic:100000`.
- **Budgets:** camera-only frames meet the 120 fps criterion at N = 10⁵ on the reference PC, with no regression against the #91 baseline of `synthetic:100000` on integrated graphics. Label overlay time is reported separately.
- **Never on the frame path:** IFC structural reading, `display::build`, sweeps, result fetches and builds, matching, clash. Between frames only uniform writes, draw-range changes, units-texture writes and small `COPY_DST` buffers, after a worker publishes.
- **Debug counters** count buffer uploads per frame; tests assert that scale sliders, case switches and tolerance changes cause none on the model buffers.

## Phases

Each phase is **one mergeable PR** that compiles, passes CI on its own, and lands in a release. Every PR also meets the standing exits: `cargo test --release --locked`, `cargo test -p bim-structural -p bim-clash`, `cargo check --release --locked --target wasm32-unknown-unknown --lib --bin open-3d-viewer`, and the 120 fps criterion on the existing bench scenarios. The branch stacks on `feat/selection-visibility` (#91) until it merges, then on `main`.

Dependencies: S1 → S2 → S3 → S4. S5 needs S2 and S3. S6 needs S3. S7 needs S4. S8 needs S7. S9 needs #91 merged. S10 needs S2 (its colours need S9). S11 needs S10. S12 needs S6. S13 needs S12 and #79 rebased on #91. S14 needs S5 and #79. F1 needs S2 and runs in parallel. S2 and S3 ship in the same release (S2 alone draws nothing).

### S1: the `bim-structural` crate

- Types, conventions, `IdKind`/`sam_id`, support and hinge preset tables with `classify` and `expand`, `sam-json` v1 serde, generated schema, `exchange::validate`, the `sam-check` binary, `docs/sam-json-v1.md`.
- `section::outline`, `section::props`, `section::cardinal_anchor`; member frames.
- Fixtures: `portal_frame.samjson` (4 nodes, 3 members, HEB 200 and IPE 300 at CP 8, one fixed and one named `roller_x` support, one hinge, one UDL case, one result set with inline blocks).

**Exit:** round trip `read(write(m)) == m`; the schema stale test passes; `sam-check` accepts the fixture and rejects a mutated copy per rule (one test per rule); presets classify every table entry back to its name; section properties of rectangle, circle and I agree with closed forms within 0.5 %; cardinal anchors 1-19 for IPE 200, UPN 200 and L 100×10 match hand values within 1e-9 m; the crate builds for wasm32.

### S2: model plumbing and the `.samjson` loader

- `Loaded.structural`/`origin`; `Model.structural: Vec<StructuralPart>` and `Model.origins`; bounds include structural nodes; `load_stack` fails only when layers and parts are both empty; `combine` carries parts with #91 keys and translates under 10 km.
- `StructuralIndex` (lazy props, `unit_key`, `element_bounds`), structural units in `ExplodeIndex`, `element_props`/`element_json`/`find_elements` fallbacks.
- `src/loaders/sam.rs` (all targets). MCP `list_structural`, `get_structural_issues`, `get_structural_element`, `find_structural`.

**Exit:** a nodes-only `.samjson` opens on the desktop and in the browser and fits to view; `get_element` and `find_elements` return structural objects; `combine` of two models keeps every structural id and both origins, and a 5 km offset aligns within 1 mm; MCP box select (`units_in_box`) returns node and member ids; the 194 MB IFC loads in unchanged time; the #91 baseline is unchanged.

### S3: render core

- `StructUniforms` at `@group(1)`, the `seg` pipeline (axes), the `glyph` pipeline with the node glyph, units on every instance, `GpuStructural` uploaded in `upload_model` beside `datum`, `StructuralDisplay` built on the load worker.
- Screen-space pick and hover; `--bench structural:N`.

**Exit:** `structural:100000` alone and with `synthetic:100000` meets the 120 fps criterion on the reference PC with no integrated-graphics regression; WebGL2 renders the same picture as WebGPU (screenshot diff within tolerance); clicking, Ctrl-click, box select, hide, isolate and undo work on nodes and members; picking a node within 8 px works with the section box on.

### S4: symbols, panel and labels

- The glyph library (supports, hinges, line supports, springs, rigid offsets), the vertex-shader LOD, *Axes on top*, the 2D variants.
- The Structural tab (show flags, model notes), `set_structural_display` (show flags), the bounded labels overlay.

**Exit:** every support and release in the fixtures draws its symbol in its frame, including named presets and a skewed support; glyph size stays within ±1 px of the setting from 1 m to 1 km; labels never exceed 256 at rest or 32 in motion; `structural:100000` with all symbols and *Numbers* labels meets the 120 fps criterion.

### S5: IFC4 structural reader

- `bim-structural` feature `ifc`: reading steps 1-10 and 14 (models, nodes, point/line/surface conditions, members, releases, implied releases, eccentricities, sections incl. tapering and `Polygon` conversion, surfaces, links) and `ifc::physical_axes`.
- Loader integration with the shared `Arc<EntityIndex>`, ribbon suppression, diagnostics into model notes.
- Hand-authored fixtures in `crates/bim-structural/tests/fixtures/ifc/`: `portal_frame.ifc` (IFC4), `skewed_roller.ifc`, `spring_cantilever.ifc`, `line_supported_slab.ifc`, `eccentric_beam.ifc` (CP 8, a non-zero `Position`, `IfcRelConnectsWithEccentricity`), `pin_joined_truss.ifc`, `legacy_2x3.ifc` (−1 stiffness, no `Axis`, `IfcStructuralLinearAction`), `millimetre_units.ifc`, `alignment.ifc` (a physical `IfcBeam` with a structural member on its axis). Each has an `.expected.samjson`.

**Exit:** every fixture reads to its expected document (compared after rounding to 1e-9); the IFC2X3 and IFC4 portal frames give equal documents apart from `source`; in `alignment.ifc` the analytical node equals a mesh vertex of the linked beam within 1e-6 m; a structural IFC opens without ribbons, with axes, supports and releases; the 194 MB non-structural IFC loads at most 3 % slower.

### S6: profile solids

- `insertion::resolve`, `solid::sweep` (straight, tapered, arc, polyline), caps with `earcutr`, surface slabs, placeholders, refusals, `JointSetback`, *Ignore cardinal point*, the `Analysis members` layer, *Members: axes / solids / both*, offset notes in properties.

**Exit:** every `Shape` sweeps to a closed, outward mesh (property test over random valid dimensions); volume equals A·L within 1e-9 relative; a CP 8 HEB 200 has its top face exactly on the axis; `eccentric_beam.ifc` draws at the expected offset with exactly one `double-eccentricity-suppressed` note; a 90° `section_rotation` swaps the bounding-box extents; the section box draws cut fills through member solids and measure snaps to them; 10⁵ member solids build in under 2 s on a desktop worker with no camera frame over 8.33 ms meanwhile; `.samjson` solids appear in the browser.

### S7: loads

- IFC reading steps 11-12; loads from `.samjson`; arrows, trapezoids, moments, temperature and displacement marks; per-case draw ranges; load scale; load labels; MCP `list_load_cases` and the load part of `set_structural_display`.
- Fixtures: a beam with a linear and a projected load, a polygonal and an `EQUIDISTANT` action.

**Exit:** each fixture's arrows and outlines sit at the expected positions and intensities (the projected load draws over the projected length with the true intensity in properties); switching load cases takes effect in the next frame with zero model-buffer uploads (debug counter); `structural:100000` with a load case shown meets the 120 fps criterion.

### S8: results

- IFC reading step 13; `.samjson` inline and sidecar blocks; `Lazy` payloads and `ResultSource`; diagrams with tension side, zero-crossing split and jumps; auto scale; legend; reactions; deformed shape; LRU; MCP `list_result_sets`, `get_member_results`, `get_node_results`.
- **Sign gate:** IFC result diagrams ship with the "not verified" note unless the sign convention has been confirmed on permitted reference exports from at least two FE tools; the confirmation (or its absence) is recorded in `docs/`.

**Exit:** a simply supported beam fixture under 5 kN/m over 6 m shows My = qL²/8 = 22.5 kNm at mid-span on the +z side, for both a z-up and a z-down member frame; a fixture with a point load and a hinge draws its jumps vertically; switching a result set on 10⁵ members takes under 200 ms on desktop and never builds during camera motion; dragging the diagram scale causes zero buffer uploads; `structural:100000` with a diagram meets the 120 fps criterion.

### S9: tint bits

After #91 merges. `UNIT_TINT_SHIFT`, the 16-entry palette at `@group(0) @binding(2)`, the mix in `fs_lit`, `fs_glass` and the structural shaders; an internal API `tint(keys, palette_index)` with undo-free semantics (status colour is view state, not history).

**Exit:** tinting 10⁵ units takes effect in the next frame; tint, hidden and selected coexist on one unit (unit tests over all combinations); the #91 baseline is unchanged on the reference PC and integrated graphics.

### S10: node matching

- `bim_structural::matching` (all alignments, cylinder mode, project to axes, members), the *Compare nodes* panel, curve, chips, table, deviation vectors, CSV/JSON export, MCP `match_nodes`, `get_node_matches`, `clear_node_matches`; IFC physical-axis node sets in the picker.

**Exit:** property test: the grid search equals brute force on 10⁴ seeded random points over 50 seeds, including points on cell borders and distances equal to the tolerance; `Auto` recovers a known rigid transform within 1e-6 m and 1e-6 rad; 10⁵ × 10⁵ nodes match in under 200 ms on a desktop worker; `shifted_frame.samjson` (portal frame shifted by (4, −3, 0) mm, one extra node, one coincident pair) against `portal_frame` reports all nodes matched at 10 mm, all unmatched at 1 mm, one unmatched B and one coincident pair, with deltas within 1e-12 of the shift; moving the slider on 5·10⁴ nodes recolours within 100 ms and keeps the 120 fps criterion.

### S11: Revit physical axes

R1 in `src/loaders/rvt.rs`: column axes (exact), framing and brace axes (approximate until the curve driver field is mapped), sections from type names and parameters, links; the sets appear in *Compare nodes*.

**Exit:** on an OpenAEC-owned structural `.rvt`, column axis ends equal the reader's f64 level elevations plus offsets within 1e-9 m; a Revit model and an IFC analysis export of the same structure give a node report the team's structural engineer signs off, with all column nodes matched at 25 mm.

### S12: the clash engine

`crates/bim-clash` (broad and narrow phase, rules, depth, deterministic ids, deviation report, features `exact` and `parallel`), `src/clash.rs`, MCP `run_clash`, `list_clashes`, `show_clash`, `get_clash`, `get_deviations` in both products.

**Exit:** hand-made fixtures classify exactly: two overlapping boxes (hard), touching faces (touch, hidden at 1 mm), a box inside another (contained), boxes 9 mm apart under a 10 mm clearance (clearance), two members sharing a node (excluded; `Joint` under `Report`), a CP 8 HEB 200 hitting a duct box above it (hard) while the same member at CP 2 does not, an analytical member and its linked `IfcBeam` (deviation, not clash); the portal frame's joints give no hard clashes; ids are identical across two runs; the engine without features builds for wasm32; a 2·10⁴-element IFC against a 5·10³-member analysis model runs in under 30 s on the reference PC with the camera at the 120 fps criterion.

### S13: clash in Open BIM Validator

After #79 is rebased on `main` with #91. The *Clash* tab, `clash-rules.json`, statuses, the `bim-bcf` viewpoint extension, `--clash`, the `select_finding` rewrite, MCP `set_clash_status` and `export_bcf`.

**Exit:** a BCF written by `--clash` opens in an independent BCF tool with camera, both components selected and coloured; topic ids are stable across runs; `select_finding` on 10⁵ elements is O(1) per finding; the validator UI keeps the 120 fps criterion during a run.

### S14: structural model checks in the validator

`SAM-01..12` in `bim-validator-core` through `bim_structural::checks`, findings with BCF export.

**Exit:** each rule has a positive and a negative fixture; the portal frame passes all rules; findings frame the affected structural objects in the 3D view.

### F1 (parallel): the FE-package loader

`crates/sam-abi-mock`, `src/loaders/fe_dll.rs` (desktop, cfg-gated), lazy result blocks through `ResultSource`, the conformance fixtures from the clean room in CI.

**Exit:** every conformance fixture opens identically as `.samjson` and through the mock; a 100-result-set model opens in under 5 s with results fetched on first use; a missing library or wrong ABI version gives a log message and no crash. When the real library exists: `sam-check` passes on its output, and a model delivered both as IFC export and through the library matches 100 % of its nodes at 1 mm and clashes identically.

### Later

Revit analytical model (R2, after the discovery run and the licence decision); IFC structural reader on wasm; IFC structural export once Phase 3's retained-graph save exists; surface results; BCF import with camera and status; offscreen snapshots in BCF; a GPU text atlas if the label cap hurts; a clash panel in the viewer product.

## Test strategy (summary)

- **Crate unit tests** (no GPU): frames (vertical rule, re-orthogonalisation), presets and `expand`, sign conversion (a `TensionPlusY` source gives the same diagram side), projected-load conversion, section properties against closed forms, cardinal anchors, `insertion::resolve` for every policy, sweep invariants, diagram polygon split, matching against brute force, Kabsch, clash fixtures, UUID determinism.
- **IFC fixtures** authored by us (by hand or generated with an LGPL toolkit used as a tool, not a dependency), each with an `.expected.samjson`. No third-party or FE-vendor files.
- **Conformance:** `sam-check` on every `.samjson` fixture and on the clean room's output; the mock DLL serves the same fixtures.
- **Viewer tests** (headless): analysis-only models open; bounds include nodes; combine keeps ids and origins; hide, isolate, box select and undo on structural elements; tint bits coexist with flags; picking prefers a node over a member within 8 px; buffer-upload counters for selector and slider changes.
- **GPU tests** (`#[ignore]`, as `src/shell.rs:1046`): offscreen screenshots of the glyph fixture against stored images on native WebGPU; the same scene in the web build compared on WebGL2 per release.
- **Benches:** `structural:N`, `synthetic:N` and the large IFC, results added to `docs/performance/baseline.md`.
- **Clean-room hygiene:** a CI allow-list of file types under `tests/`.

## Constraints

- Rust, iced 0.14, wgpu (WebGPU and WebGL2); the same code on the desktop and in the browser; desktop-only parts behind `cfg(not(target_arch = "wasm32"))`.
- Licence LGPL-3.0-or-later; new dependencies MPL-2.0, MIT, Apache-2.0 or ISC, checked per dependency. No GPL or AGPL (no CGAL bindings).
- PR #91's types and flags are the base.
- Clean room for the FE-package format: this repository sees only the ABI and `sam-json` output.
- Repository rules: content in English, commands with Dutch and English labels, `README.md` and `README.nl.md` updated together, product names only as format names.

## Risks

| Risk | Effect | Mitigation |
|---|---|---|
| Few real IFC4 analysis-model samples; exporters differ on cardinal point vs profile position, `Axis` and result encoding | Wrong offsets or diagrams on real files | Authored fixtures first; collect permitted real exports early (S5); one precedence function; offset notes and *Ignore cardinal point* |
| IFC does not state the internal-force or reaction sign convention | Diagrams on the wrong side | Canonical convention, recorded source convention, the S8 sign gate, a side toggle |
| Semantic-path coordinates differ from ifc-lite's mesh frame (RTC, site transforms) | Nodes off the physical meshes; false matches and clashes | Take the mapping from `result.frame` only; `alignment.ifc` test in S5 |
| Double eccentricity | Solids offset by a profile depth; false clashes | `Insertion` split, `EccentricityPolicy`, `SAM-12`, notes |
| Merge order of #91, #79 and this branch (`app.rs`, `Cargo.toml` workspace, selection) | Rework | Stack on #91; engine and MCP before the validator UI; S13 after #79's rebase |
| Canvas label cost | 120 fps missed | Caps in motion and at rest, at-rest cache; GPU atlas if needed |
| `MAX_UNITS` on large combined models | Units run out | Only nodes, members and surfaces get units; the modeller design's open item for larger textures |
| Result volume (10⁵ members × 100 sets × 20 stations × 6 × 8 B ≈ 9.6 GB) | Memory | Lazy per-set fetch, LRU, sidecar blocks |
| `ifc-lite-geometry` on wasm (hard `rayon` dependency) | Browser features missing | Sweeps and the clash core do not depend on it; IFC import stays desktop-only |
| Joint overlaps flood clash lists | Unusable results | Shared-node exclusion with a joint radius; `Joint` reported only on request; *HalfDepth* option |
| `rvt-dll` licence tag and branch pin; unknown analytical classes | Revit analytical delayed | R1 needs no new class work; R2 after the licence decision; R3 as ground truth |
| f32 display alignment over large origin differences | Misdrawn federation | Align on Add only under 10 km; matching and clash in f64 regardless |
| Profile catalogue data under database right | Cannot bundle tables | Dimensions from the file only; placeholders otherwise |
| Clean-room contamination | Legal | Only the ABI and `sam-json` cross the boundary; review checks it; CI file allow-list |

## Calls made in writing this design

These were settled by the author of this document, not by the project owner, and are open to correction:

- Structural types live in a new headless `crates/bim-structural` now, written to fold into `bim-core` in Phase 3, instead of waiting for it.
- Structural objects share the model's `u64` id space and #91's `ElementKey`/`Pickable::Element`; no new pickable variant. Only nodes, members and surfaces get units; decorations, including addressable loads, use their target's unit.
- A structured f64 origin per source model and a bounded display alignment on *Add* (under 10 km) are pulled forward from Phase 3.
- Match and clash colours are tint bits 4-7 in the units texture with a 16-entry palette, in their own PR after #91.
- Structural render state is a separate `StructUniforms` bind group; the shared `Uniforms` is not grown.
- Supports and hinges are lit 3D glyphs at screen size with a world cap and vertex-shader LOD.
- Named support and hinge types expand through one documented preset table; unknown names stay `Custom` and free, never guessed. `PIN_JOINED_MEMBER` implies hinges at both ends when no explicit conditions exist.
- Member solids are an ordinary mesh layer derived from the document; the clash engine reads the f64 copy of the same evaluator output. The sweep uses the crate's own outlines and `earcutr`; `ifc-lite-geometry` is used for profiles only at IFC import.
- Default eccentricity policy for IFC: a cardinal point cancels the profile `Position` translation; the FE DLL delivers explicit offsets.
- Cardinal points are IFC4's 1-19 only.
- Default diagram side is tension; default clash joint rule excludes members sharing a node; an analytical member against its own physical element is a deviation, not a clash.
- Results are stored in f64 and fetched lazily per set; display uploads f32.
- `sam-json` v1 is the C-ABI payload, a file format (`.samjson`) and the fixture format; ABI symbols are `sam_*` and the library reports its own extensions.
- The node-matching tolerance and the clash tolerance are separate settings.
- The clash UI is validator-only; the engine is shared and reaches the viewer through MCP.
- IFC structural export is out of scope until Phase 3.

## Open questions

1. **Sign convention** of IFC4 internal forces and reactions: which permitted reference exports (at least two FE tools) confirm it before S8 ships without the "not verified" note?
2. **Eccentricity precedence** in real exports: is "cardinal point cancels the `Position` translation" right, what do `IfcMaterialProfileWithOffsets` values mean in practice, and is `IfcRelConnectsWithEccentricity` applied together with the cardinal point?
3. **Linear superposition:** may the viewer combine load-case results into a combination the file does not hold when `IsLinear = true`, or is that computing and out?
4. **Revit analytical classes:** which OpenAEC-owned structural `.rvt` can be used for the discovery run, and is the live export through the Revit plugin accepted as ground truth?
5. **`rvt-dll` licence and branch:** change `LicenseRef-Proprietary` and pin a `main` commit before structural code is added there?
6. **Clash semantics:** clearance in S12 or later; touches reported; self-clash within one model allowed by default?
7. **Clash panel in the viewer product,** or validator-only with MCP in the viewer?
8. **Profile catalogue:** decided 2026-11-06: a hand-entered subset of the common European rolled sections (IPE, HE A/B/M, UPN, UPE, L), dimensions only, typed from the standard dimension tables, used when a file gives a section name without dimensions; the file always wins.
9. **IFC on wasm:** enable `ifc-lite-core`/`-geometry` for wasm32 so analysis IFCs open in the browser?
10. **IFC2X3 analysis models:** full support or read with diagnostics (this design)?
11. **Surface results:** which phase, given IFC carries no FE mesh topology?
12. **Labels:** is the bounded canvas overlay enough, or is a GPU text atlas wanted early?
13. **Naming of the clean-room crate** in anything committed here (`rfem-dll` in the request; this design uses `fe_dll` and `sam_*`), and whether `sam-json` is published as an OpenAEC interchange spec of its own.

## Left to the builder

- Exact glyph meshes, triangle counts, pixel sizes and the colour palette (x red, y green, z blue kept; theme tokens for light and dark).
- The label declutter grid and caps within the stated limits, tuned by the bench.
- Whether the active structural buffers are one buffer with ranges or one per primitive.
- The BVH build heuristics and leaf sizes in `bim-clash`.
- The LRU sizes for result selections.
- How the Structural tab and *Compare nodes* panel dock next to the existing panels, and the names of commands and shortcuts, following `src/shell.rs`.
- Whether `display::build` lives in `bim-structural` behind a feature or in `src/structural/`.
