# Smarter STEP: faces, holes and sheet metal unfolding

**Status:** proposal, 2026-11-06. Nothing in this document is built yet. Branch `feat/step-sheetmetal` (from `main` at `032e130`, after PR #91 and the structural S3 merge).

**Request.** "STEP als in wat meer intelligente dingen van unfold en dat soort dingen" and "make sure it works": the viewer should understand what a STEP part *is* (faces, holes, sheet thickness, bends) and unfold sheet metal parts into flat patterns, not only draw triangles.

**Relation to other work.** The parallel branch `feat/freecad-formats` adds FCStd, BREP and IGES readers that produce the same `cadmpeg-ir` model. Everything below is built on `cadmpeg-ir` through one format-neutral module (`src/brep/`), so those readers get faces, holes and unfolding by calling one capture function. The conventions of the modeller and structural designs (document vs display, no CPU work per camera frame, element ids and unit flags of PR #91, wasm builds) apply here too.

**Naming.** Only file formats and public standards are named (STEP, DXF, DIN 6935). Code, UI and MCP tools use neutral names.

## Intent

A mechanical engineer, a sheet metal shop or a steel fabricator opens a supplier's `.step` file and asks practical questions: how thick is this plate, what diameter are these holes and do they go through, what is the radius of this fillet, and what does the blank look like before it is bent. Today the viewer answers none of them. It keeps the triangles of every face and throws the exact geometry away (`src/loaders/step.rs:156-185`). The measure tool guesses faces from triangle normals (`src/measure/geometry.rs:205`) and does not know that a cylinder is a cylinder.

The aim:

1. **A click picks a face or an edge**, and the properties panel shows what it is: plane with its normal, cylinder with its radius, diameter and axis, cone with its angle, its area; an edge with its type and length.
2. **Recognition.** Every part gets a list of its holes (diameter, depth, through or blind, axis) and its fillets and rounds. Two picked faces show their distance and angle.
3. **Sheet metal.** A part of constant thickness with cylindrical bends is recognised as sheet metal. It shows its thickness, a bend table (angle, inner radius, up or down, bend allowance and deduction) and its flat pattern, computed with a K-factor. The flat pattern is drawn next to the part and exported as DXF with outline, holes and bend lines on their own layers.
4. **MCP.** Every one of these answers is an MCP tool, so an agent can ask "which parts in this assembly are sheet metal, and export their flat patterns".

The numbers come from the exact B-rep (planes, cylinders, circles), not from triangles.

## Goals

- **Face and edge picking on STEP parts**, with exact properties, without slowing load or the camera.
- **Hole and fillet recognition per part**, with a holes list in the properties panel and over MCP.
- **Sheet metal detection that does not lie.** A part is called sheet metal only when the evidence holds (constant thickness, paired faces, coaxial bend pairs). A machined block, a pin or a bracket of two thicknesses is not, and the panel says why.
- **A correct flat pattern.** Exact for cylindrical bends under the K-factor model. The formulas are stated here and checked by tests against hand-computed values.
- **DXF export** of the flat pattern that CAM and laser software reads (DXF R12 ASCII, LINE, ARC and CIRCLE only, millimetres).
- **Desktop and browser.** Everything compiles for wasm32. In the browser the analysis runs on demand for the selected part.
- **Format-neutral.** `src/brep/` depends on `cadmpeg-ir` and `cadmpeg-tessellate` only, not on the STEP codec.

## Non-goals

- **Editing.** No re-bending, no changing the K-factor of a file, no writing STEP.
- **Feature trees.** STEP carries no construction history. We recognise from geometry. If a future decoder fills `ir.model.features` (`cadmpeg_ir::features::FeatureOperation::SheetMetalBend` and similar exist in the IR), those may serve as hints later. This design does not depend on them.
- **Forming features** (louvres, embosses, ribs, formed countersinks), **hems** (bend angle above 175°), **conical bends**, **rolled cylinders** and **closed tubes** are not unfolded. They are detected and reported as the reason a part is not unfolded.
- **Manufacturing data.** No gauge or bend tables from a shop, no tooling, no nesting.
- **PMI.** Cosmetic threads, tolerances and hole callouts in AP242 are not read in this design.

## Anti-goals (failure even if it technically works)

- **A wrong flat pattern shown as right.** If any face on the unfold side is not a plane or a cylinder, or the bend graph is inconsistent, there is no flat pattern. The part gets a reason instead. A pattern with overlapping regions is shown in a warning colour and marked "overlaps" in the panel, the DXF and MCP.
- **Sheet metal false positives.** The test suite contains parts that must not be detected, and the detector has explicit thresholds (see *Detection*).
- **CPU work per camera frame.** Highlights and flat patterns are GPU buffers built when the selection or a toggle changes, never on a camera move.
- **Keeping the whole `CadIr` alive.** A 300 MB STEP decodes to several GB of IR. We keep a compact summary of each body, built while tessellating, under a memory cap.
- **A second triangulation.** The summary reuses the face meshes and coedge traces the tessellator already makes (`cadmpeg_tessellate::FaceMesh::coedges`).
- **Breaking PR #91.** Face picking is a mode of the existing pick. Element selection, hide, isolate, undo and units flags stay as they are.
- **Licence leaks.** No new dependencies. The DXF writer is hand-written. Earcut comes from the existing `earcutr`. No third-party unfold code is read or copied. The maths is public engineering knowledge (K-factor, bend allowance and deduction, DIN 6935).

## Where the viewer stands today

Line numbers refer to `main` at `032e130`.

- **STEP loader** (`src/loaders/step.rs`). `StepCodec` decodes into `CadIr` (line 65). `convert` (236) maps faces to bodies through shell, region and body (239-246). It builds a `Tessellator` with chord `size·1e-3` clamped to 0.01–10 mm, angle 0.35 rad and at most 10 000 vertices per face (292). `Scene::mesh` (156) tessellates each body once into a body-local mesh in millimetres, shared by `Rc` between occurrences. `Mesh::push` appends faces one after another, so **the triangles of one face are contiguous in the body mesh**. `Scene::emit` (188) places a body mesh into a layer with `scale(0.001)·world`, flips the winding for mirrored placements, and records the element range `(id, first index, count)`. The IR is dropped when `convert` returns. Only the triangles survive.
- **IR units.** `cadmpeg-ir` stores lengths in millimetres (`cadmpeg_ir::units` module doc) and angles in radians.
- **IR topology** (`cadmpeg-ir 0.6.8`, `src/topology.rs`). `Body → Region → Shell → Face`. `Face { surface, sense, loops: FaceLoops::{Classified{outer, inner}, Unspecified} }`. `Loop { boundary: LoopBoundary::Ring(LoopRing{coedges}) | Vertex }`. `Coedge { edge, sense, radial_next, pcurves }`, where `radial_next` is the partner use of the same edge, which gives face adjacency. `Edge { carrier: EdgeCarrier, start, end }`. `Vertex { point }`.
- **IR geometry** (`src/geometry.rs`, `src/geometry/analytic.rs`). `SurfaceGeometry::{Solved(SolvedSurfaceGeometry), Procedural{cache}}`. `SolvedSurfaceGeometry::{Plane, Cylinder, Cone, Sphere, Torus, Nurbs, Polygonal, Transformed(PlacedSurface), Unknown}`. `PlaneSurface { origin, frame }`, `CylinderSurface { origin, radius, frame }`, `ConeSurface { origin, radius, ratio, half_angle, frame }`, `TorusSurface { center, major_radius, minor_radius, frame }`. Curves: `SolvedCurveGeometry::{Line, Circle, Ellipse, Parabola, Hyperbola, Degenerate, Composite, Nurbs, Polyline, Transformed, Unknown}`. `Transformed` wraps a basis in an affine transform up to `MAX_GEOMETRY_NESTING`.
- **Tessellator** (`cadmpeg-tessellate 0.6.8`). `Tessellator::face(&FaceId) -> FaceMesh { positions, normals (oriented with the face), triangles, coedges: Vec<CoedgeTrace { coedge, uv, points }>, deviation }`. Edge samples are shared between the faces that use an edge, so neighbouring faces meet at the same points.
- **Model** (`src/model.rs`). `Layer` (15) has `elements: Vec<(u64, u32, u32)>` and `element_at` (68). `Model::new` (516) runs `unshare_element_vertices` (810), which keeps triangle order, and `split_layer` (731), which keeps triangle order and splits element ranges. `Model::combine` (408) appends layers. `Loaded` (`src/loaders/mod.rs:234`) carries layers, views, project, structural and origin.
- **Picking and measuring.** `App::pick` (`src/app.rs:1373`) casts a ray against triangles and selects the element. The measure tool snaps to corners, edge middles, feature lines found by dihedral angle (`LayerTopology`, `src/measure/geometry.rs:26`) and faces grown by normal similarity. `Entity::{Point, Line, Plane}` (322) feeds `perpendicular` and `angle_between`. `SnapKind::{Corner, Middle, Edge, Face}` (`src/measure/hover.rs:85`).
- **Properties panel.** `App::properties_body` (`src/app.rs:2091`) shows bilingual sections (`l.text(nl, en)`). Tables under the 3D view already exist for schedules (`src/app/schedules.rs:164`).
- **Renderer.** `ScenePrimitive` (`src/viewport/render.rs:102`) carries the model and `Arc`s that are uploaded only when the pointer changes (structural marks). Extra line geometry exists for datum grids (`datum`, 198, built in `upload_model` at 584).
- **Workers.** Desktop work runs on threads through `Task::perform` (`edges_task`, `src/app.rs:420`). The browser has no worker threads. Work is deferred to after the next frame (`src/app.rs:436`).
- **MCP** (`src/mcp.rs:84 tools()`). Names are snake_case verbs: `get_element`, `find_elements`, `list_structural`, `get_structural_element`, `measure_between_elements`, `save_file`. The commands are executed by `src/remote.rs` (dispatch at 434, `element_json` at 1008).
- **Export** (`src/export/mod.rs`). `Writer` trait and `write_file`, which writes to `<path>.part` and renames. The browser download goes through `crate::web::download` (`src/app.rs:528-540`).

## How engineers use it

Each session below is an exit test of the PR (see *Test plan*).

1. **What is this face?** Open `bracket.step`. In the ribbon, *Select* switches from *Part* to *Face*. Hovering lights up the face under the cursor. A click selects it, and the panel shows: *Cylinder, R 2.00 mm (Ø 4.00), axis (0, 1, 0) through (12.0, 0, 4.0) mm, area 125.66 mm², bend 1 inner*. Ctrl-click a second face: *Between faces: parallel planes, distance 2.00 mm, angle 0.0°*.
2. **Which holes?** Select the part (Part mode). The panel has *Holes (6)*: *4 × Ø 6.60 through, depth 2.00* and *2 × Ø 4.20 blind, depth 8.00, drill point 118°*. A click on a row selects those hole faces and frames them.
3. **Unfold.** The same part shows *Sheet metal: t 2.00 mm, 3 bends, K 0.44*. The toggle *Flat pattern* draws the blank in the plane of the base flange, beside the part, with bend lines (up blue, down orange). *Bend table* opens a table under the 3D view: bend, angle, included angle, inner radius, direction, length, BA, BD. *Export DXF…* writes `bracket-flat.dxf`. Opening it in the viewer shows layers OUTLINE, HOLES, BEND_UP and BEND_DOWN.
4. **Not sheet metal.** A machined block shows *Sheet metal: no (thickness 20.00 mm is not thin for a part of 100 × 50 mm)*. A two-thickness bracket shows *no (thickness varies: 3.00, 5.00 mm)*.
5. **Agent.** `get_sheet_metal {element: 12}` returns thickness and bends. `export_flat_pattern {element: 12, path: "flat/12.dxf"}` writes the DXF.

## Architecture

### Modules

```
src/brep/
  mod.rs        BrepStore, BrepBody, faces/edges/loops summary, face runs, memory accounting
  capture.rs    cadmpeg-ir + FaceMesh -> BrepBody (called by the STEP loader, later FCStd/BREP/IGES)
  surface.rs    resolved analytic surfaces and curves (Transformed/Procedural cache unwrapped)
  features.rs   planar faces, holes, fillets and rounds; face-face relations
  sheet.rs      sheet metal detection, bend graph, unfold, bend table, flat pattern
  dxf.rs        DXF R12 writer for flat patterns (also used by export_flat_pattern)
  tests.rs      fixtures (STEP writer moved here from step.rs tests) and tests
src/app/brep.rs pick mode, panel sections, overlay building, analysis tasks
src/viewport/render.rs  overlay buffers (highlight triangles and lines, flat patterns)
```

`src/brep/` does not depend on `iced`, `wgpu` or the app. It compiles for wasm32 and is unit-testable headless.

### Data kept per body (`BrepBody`)

Built in `Scene::mesh` while tessellating: for each face, the triangle count before and after `Mesh::push` gives its run, and the `FaceMesh` gives samples and coedge traces. Lengths are in millimetres, body-local.

```rust
pub struct BrepStore {
    pub bodies: Vec<Arc<BrepBody>>,                 // one per IR body, shared by its occurrences
    pub elements: HashMap<u64, Vec<Placed>>,        // element id -> bodies placed in it
    pub dropped: usize,                             // bodies without summary (memory cap)
}
pub struct Placed {
    pub body: u32,
    pub to_world: DMat4,     // body mm -> world metres, as Scene::emit uses (before the model matrix)
    pub face_offset: u32,    // element-local face number of this body's face 0
    pub edge_offset: u32,
}
pub struct BrepBody {
    pub kind: BodyKind,                  // solid, sheet, ...
    pub faces: Vec<BFace>,
    pub loops: Vec<BLoop>,               // BFace::loops ranges into this
    pub uses: Vec<(u32, bool)>,          // coedge uses: (edge index, reversed); BLoop ranges into this
    pub edges: Vec<BEdge>,
    pub vertices: Vec<DVec3>,
    pub samples: Vec<DVec3>,             // polylines of edges that are not lines or circles
}
pub struct BFace {
    pub surface: Surf,
    pub sample: (DVec3, DVec3),          // a point inside the face and its outward normal (from the mesh)
    pub area: f64, pub centroid: DVec3, pub bbox: (DVec3, DVec3),
    pub loops: Range<u32>,               // the outer loop first when the IR classifies it
    pub triangles: u32,                  // 0: not tessellated (still analysed)
}
pub enum Surf {
    Plane { origin: DVec3, normal: DVec3 },                     // normal as stored (sense applied separately)
    Cylinder { origin: DVec3, axis: DVec3, radius: f64 },
    Cone { origin: DVec3, axis: DVec3, radius: f64, half_angle: f64 },
    Sphere { centre: DVec3, radius: f64 },
    Torus { centre: DVec3, axis: DVec3, major: f64, minor: f64 },
    Other(&'static str),                                        // "B-spline", "offset", ...
}
pub struct BEdge {
    pub curve: Crv,                      // Line, Circle{centre, axis, radius}, Ellipse{..}, Other(&str)
    pub ends: [u32; 2],                  // vertex indices
    pub faces: [u32; 2],                 // the two faces (u32::MAX on an open edge)
    pub length: f64,
    pub samples: Range<u32>,             // empty for lines and circles (evaluated exactly)
}
```

- **Resolving geometry.** `Transformed` chains are composed into one affine map and applied to origin, axis and radius. A non-uniform scale turns a circle into an ellipse, which becomes `Other`. `Procedural { cache }` uses the cache. Anything else is `Other`.
- **Outward normal.** The tessellator orients `FaceMesh.normals` with the face. `sample` takes the centroid of the face's largest triangle and its geometric normal, and all convex or concave decisions use it. This does not depend on how a writer used `sense`.
- **Edge length.** Line: `|end − start|`. Circle: `radius · swept angle`, with the angle taken from the endpoints in the edge's direction (2π for a closed edge). Otherwise the sum of the polyline from the coedge trace.
- **Area.** Planar faces: shoelace area of the loops, projected into the plane, with circle edges evaluated exactly (holes subtracted). Curved faces: sum of the face's triangles, shown with "≈".
- **Adjacency.** `BEdge.faces` comes from coedges: each coedge's loop belongs to a face, and two coedges of one edge give the two faces. That is the face adjacency graph that features and sheet metal use.

### Triangle → face: face runs

`Layer` gets `face_runs: Vec<(u32, u32)>`, sorted: (first triangle index in the layer, element-local face number). A run lasts until the next run starts. `Scene::emit` pushes one run per face per placement. `split_layer` rebuilds the runs from its `placed` table, and `Model::combine` shifts nothing because layers are appended whole. `Model::face_at(layer, triangle) -> Option<(u64, u32)>` binary-searches the runs. The cost is 8 bytes per *placed face*, not per triangle. A mirrored placement keeps triangle order, so runs do not change.

`BrepStore` lives on `Loaded.brep: Option<BrepStore>` and on `Model.brep: Option<Arc<BrepStore>>`. `Model::combine` merges stores and remaps element ids with the id mapping it already uses.

### Memory bound and laziness

- The summary costs about 120 bytes per face, 80 per edge and 24 per vertex, plus samples for free-form edges only. Typical mechanical parts are planes, cylinders, lines and circles and store no samples. This is well under the f32 mesh of the same faces, which costs at least 36 bytes per vertex with normals and colours.
- **Cap:** 512 MB desktop, 128 MB browser, counted while capturing. Past the cap, further bodies get no summary (`BrepStore::dropped`, shown in project info as "STEP B-rep kept for N of M bodies"). Face picking on those falls back to triangle face regions, as the measure tool does today.
- **Analysis is lazy.** Recognition and sheet metal analysis run per unique body, once, and the results are cached in `App` by `(model id, body index)`:
  - on desktop, in a background pass on a worker after the model is shown, for at most 2 000 unique bodies or 2 s wall time. The rest run on demand.
  - on demand when a part is selected, an MCP tool asks, or the panel is opened.
  - in the browser, only on demand, after the next frame (as `edges_task`).
- Re-reading the file to rebuild a dropped body is a follow-up (*F5*).

### Display overlay

`ScenePrimitive` gets `overlay: Arc<Overlay>`: triangles and lines in world coordinates (before the model matrix, as layers are), each vertex with its colour and the unit index of its element. It is uploaded only when the `Arc` pointer changes. It holds:

- the hovered and selected faces (fill with depth bias, drawn after the lit pass with `LessEqual`, so the face sits on its own triangles) and their outline;
- the selected edge (line, 2 px where wide lines exist, else 1 px);
- hole axes of the selected part (thin lines through the hole length);
- the flat patterns switched on: fill (two-sided), outline, bend lines (up blue, down orange; dashed lines are a follow-up).

Overlay vertices carry the element's unit, so the exploded view moves them and hide or isolate hides them, as for layer vertices (`scene.wgsl`). Building the overlay is O(selected faces' triangles + flat pattern size). It runs on selection, hover or toggle changes, never on camera frames. Face triangles come from the layer (the face run gives the triangle range), so nothing is re-tessellated.

## Recognition (`brep::features`)

Tolerances: `ε = max(1e-3 mm, 1e-6 · body diagonal)` for lengths, and `δ = 0.1°` for parallel or coaxial tests (`cos` compared with `cos δ`). Axes are compared as lines: direction within δ (either sign), and the distance between the lines below ε.

### Face properties (pick, panel, `get_faces`)

| Surface | Shown |
|---|---|
| Plane | outward normal, area, centroid, perimeter |
| Cylinder | radius, diameter, axis (point closest to centroid + direction), convex or concave, angular span, axial length, area |
| Cone | half angle and included angle (2·half angle), axis, apex, convex or concave, area |
| Sphere, torus | radii, centre, axis |
| Other | type name, area ≈ |
| any | the feature it belongs to: hole n, fillet, round, bend n (inner or outer), sheet face A or B |

Edges: type (line, circle, arc, ellipse, B-spline, other), length, for circles centre, axis and radius, for lines direction. Also the two faces and their dihedral angle at the edge midpoint (convex, concave or smooth).

**Two picked faces:**
- plane–plane: angle between normals. If parallel, the distance `|(o₂ − o₁)·n₁|`.
- cylinder–cylinder: angle between axes. If parallel, the distance between the axis lines (hole centre distance).
- plane–cylinder: angle between normal and axis. If the axis lies in a parallel plane, the axis-to-plane distance.
- otherwise: the angle between the sample normals only.

### Holes

1. **Candidates.** Cylindrical faces that are concave: with sample point `p`, its foot `f` on the axis and outward normal `n`, the face is concave when `n · (p − f) < 0`.
2. **Grouping.** Concave cylinders that are coaxial, have equal radius (within ε) and are edge-adjacent form one group. CAD writers often split a hole wall into two half faces at a seam.
3. **Full turn.** The sum of the angular spans of a group must be at least 359°. If it is less, it is a partial cylinder (notch, slot end) and not a hole.
4. **Extent.** Project all loop points of the group onto the axis to get `[s₀, s₁]`. The depth of the cylindrical part is `s₁ − s₀`.
5. **Ends.** At each end, look at the faces across the edges at that end:
   - **open**: the faces across are not coaxial bottoms. A plane perpendicular to the axis with this circle as an inner loop is the usual case. A curved face (a cross hole in a tube) also counts as open.
   - **flat bottom**: a planar face perpendicular to the axis whose only loop is this circle.
   - **drill point**: a coaxial concave cone closing at its apex. Tip depth = `r / tan(half_angle)`, reported with the included angle (118° typical).
   - **step**: a coaxial planar annulus or cone leading to a larger or smaller coaxial cylinder. The hole is reported with `stepped: true`. Counterbore and countersink classification is follow-up *F2*.
6. **Classification.** Both ends open: **through**. One end open and one bottom: **blind** (depth to the bottom plane, plus tip depth if any). Otherwise: **other**.
7. **Axis.** The point is the centre of the open end, or of the first open end of a through hole. The direction points from that end into the material.
8. **List per part.** Holes are sorted by diameter, then position, and grouped for display as `n × Ø d through|blind depth`.

### Fillets and rounds

A cylindrical or toroidal face qualifies when its angular span is below 180° and it is **tangent** to its neighbours across its two longest edges: the sample normals of the two faces at the edge midpoint differ by less than 1°. A concave face is a **fillet**, a convex one a **round**, each with its radius. In sheet metal parts the same faces become bends (see below), and the sheet label replaces fillet and round.

### Planar faces

Every plane gets normal, area and centroid. Coplanar planes (same normal within δ, offset within ε) are numbered as one *plane group* so the panel can say "3 faces in this plane".

## Sheet metal (`brep::sheet`)

Notation: thickness `t`, inner bend radius `r`, K-factor `K`, bend angle `θ` (the deflection, 90° for an L, in radians in the formulas), neutral radius `ρ = r + K·t`.

### Formulas

- **Bend allowance** (length of the neutral fibre in the bend): `BA = θ · (r + K·t)`.
- **Outside setback:** `OSSB = (r + t) · tan(θ/2)` for θ < 180°.
- **Bend deduction** (mold-line convention): `BD = 2·OSSB − BA`.
- **Flat length of a simple L** with outer leg lengths `A` and `B` (mold-line dimensions): `L = A + B − BD = (A − (r+t)·tan(θ/2)) + (B − (r+t)·tan(θ/2)) + BA`.
- **K rule.** *Fixed* (default `K = 0.44`, a setting, overridable per part), or *DIN 6935*: correction factor `k = 0.65 + 0.5·log₁₀(r/t)` for `r/t < 5`, `k = 1` for `r/t ≥ 5`. DIN 6935 defines the neutral fibre at `r + k·t/2`, so `K = k/2`. For bends with an opening angle above 165° (θ < 15°), DIN 6935 sets the compensation to zero. We still use `BA` as above and note it in the bend table.
- **Reported per bend:** θ, included angle `180° − θ`, `r`, direction, bend length (along the axis), `K`, `BA`, `BD` (only for θ ≤ 170°, else empty), and the bend line in the flat pattern.

Worked check (the L-bracket fixture): `t = 2`, `r = 2`, `K = 0.44`, `θ = 90°`, `A = 50`, `B = 30`. Then `BA = π/2 · 2.88 = 4.5239`, `OSSB = 4`, `BD = 3.4761`, `L = 76.5239 mm`.

### Detection

Per body. Only bodies with `kind` Solid and a closed shell qualify.

1. **Plane pairs.** For each planar face `f` with outward normal `n` and an interior sample `x`: find planar faces `g` with `n_g · n < −cos δ` lying behind `f`: `τ = (o_g − x) · (−n) > ε`. Then project `x − τ·n` into `g`'s plane and test whether it lies inside `g` (2D point in polygon on the projected loops, with holes). Planes are bucketed by quantised normal, so this is O(P) expected. Each accepted pair gives a gap `τ`.
2. **Thickness.** `t` = the area-weighted mode of the gaps, with gaps clustered within `max(0.01 mm, 0.5 %)`.
3. **Bend pairs.** Coaxial cylinder pairs: one concave with radius `r`, one convex with radius `R`, `|R − r − t| ≤ max(0.01 mm, 0.5 % t)`, equal angular span within 1°, and overlapping axial extent. Split halves are merged first, as for holes.
4. **Thin and constant.** *Paired* faces are the planes and cylinders in a pair at `t`. Every other face is *unpaired*. `A_pair` is the area of the paired faces, both sides. The body is sheet metal iff:
   - (C1) at least one plane pair at `t`;
   - (C2) **thin**: `t ≤ 0.2 · √(A_pair / 2)`. A 100 × 50 × 20 block fails (`20 > 14.1`). A 100 × 50 × 2 plate passes as a flat blank with no bends.
   - (C3) **only edge faces besides the pairs**: every vertex of every unpaired face is also a vertex of a paired face. As a fallback for writers that add vertices, it may lie within ε of the surface of a paired face adjacent to that unpaired face. Edge faces of a sheet (outline sides, through-hole walls, bend end faces) have all their vertices on the A and B boundaries. A blind hole, a pocket, a second thickness or a forming feature has vertices that are not, and fails here. The reason names those faces, and any other gaps found in step 2 ("thickness varies: 3.00, 5.00 mm").
   - (C4) **connected**: the A side (step 5) covers at least 98 % of `A_pair / 2`.
5. **Sides.** Start at the base face (below) and grow the **A side** over edges that are *smooth* (dihedral below 1°) through paired planes and paired cylinders. The partners form the B side. Edge faces join A and B at sharp edges.
6. **Base face.** The default is the largest planar face. Of its two sides, the one from which the majority of bends go *up* is chosen. A tie goes to the larger area. The user can pick another base face (Face mode, *Use as base face*) or pass `base_face` over MCP.

Constants (`THIN_RATIO = 0.2`, `A_SIDE_COVER = 0.98`, the gap clustering tolerance and ε) are named, and are tuned on the real samples before merge.

### Unfold

The A side is a graph: nodes are A faces (planes and cylinders), and links are their shared smooth edges. Coplanar A planes joined by a smooth edge belong to one **flange**. Each A cylinder is a **bend**, which links two flanges along its two straight tangent edges (lines parallel to the axis).

**Breadth-first from the base flange.** Every flange `F` gets a rigid transform `T_F` from model space to *flat space*, where the base flange's plane is the flat plane. `T_base = I`. For a bend between a placed flange `F` and an unplaced flange `G`:

- Take the bend axis point `a`, unit direction `u` and A-side radius `r_A`: `r` if the bend goes up from A (A is the inner side), `r + t` if down.
- `e_F` is the unit radial vector from the axis to `F`'s tangent edge: `e_F = normalize(w − (w·u)u)` with `w = q_F − a` for a point `q_F` on the shared edge. `e_G` is the same for `G`.
- Signed rotation that carries `e_G` onto `e_F`: `α = atan2(((e_G × e_F) · u), e_G · e_F)`, with `|α| = θ`.
- `R(p) = a + Rot(u, α)·(p − a)`: rotation about the axis line.
- Tangent of the neutral arc at `F`, pointing into the bend: `d_F = −sgn(α) · (u × e_F)`.
- **`U(p) = R(p) + BA · d_F`** and **`T_G = T_F ∘ U`** (apply `U` first).

Because `R` rotates about the bend axis, it preserves the distance to the axis, so `G`'s A face lands in `F`'s A plane and its B face in `F`'s B plane. The translation opens the strip of width `BA` that the bend becomes.

**Bend region.** A point `p` on a bend face maps as follows. Let `w = p − a`, `s = w·u`, `w⊥ = w − s·u`, and `ψ` = the angle from `e_F` to `w⊥` measured in the bend's direction (`−sgn(α)` about `u`), with `ψ ∈ [0, θ]`. Then:

`flat(p) = T_F( a + s·u + |w⊥|·e_F + ρ·ψ·d_F )`.

At `ψ = θ` this equals `T_G(p)`, so the mapping is continuous across both tangent lines. This is the exact development of the neutral fibre, which is what the K-factor model defines.

**Cycles.** A bend whose far flange is already placed is a closing bend (welded corner, closed profile). Its predicted transform is compared with the existing one on the flange's vertices. If they agree within `10·ε`, it is accepted. Otherwise the part is "closed loop: needs a seam", and no flat pattern is made.

**Refusals.** No flat pattern is made, and the reason is given, when:
- an A face is not a plane or a cylinder (cone, torus, sphere, B-spline: non-developable or unsupported);
- θ > 175° (hem);
- a cylinder pair spans more than 180° without a flange between, or closes 360° (rolled or tube);
- two bends touch without a flange between them;
- C3 found forming features.

**Outline.** Edges of A faces whose other face is not an A face are boundary edges. Each is mapped into flat space: through `T_F` on a flange, through the bend map on a bend. Each B-rep vertex gets its flat position once, from the first face that places it, so loops close exactly. The edges are chained by vertex into closed loops. The loop with the largest absolute area is the **outline**. The others are **inner loops**: a single circle edge is a **hole** (exported as CIRCLE), anything else a **cutout**. Edges of A planes that are circles parallel to the plane stay circles or arcs. Lines stay lines. Edges on bends and free-form edges become polylines.

**Bend lines.** For each bend, the tangent lines at `ψ = 0` and `ψ = θ` and the centre line at `ρ·θ/2`, between the axial extent `[s_min, s_max]` of the bend face. Direction: **up** if the bend axis lies on the A side of `F` (`(a − q_F)·n_A,F > 0`, where `n_A,F` is the outward normal of `F`'s A face), else **down**, seen from the A side.

**Flat frame.** Origin at the bounding-box minimum. `x` along the longest straight outline edge of the base flange. `y = N × x`, with `N` the A-side normal. **Flat size** is the bounding box `w × h` in that frame. The flat area is also reported.

**Checks** run on every unfold and are reported:
- *overlap*: segment intersection between all loops, using a uniform grid, O(n) expected. Overlap means flanges collide when flat.
- *area*: the flat A area must equal `Σ A planes + Σ bends · ρ/r_A` within 0.1 %.
- *closure*: every chained loop must close.

## UI

- **Ribbon, Home tab, Select group.** A three-way switch *Part | Face | Edge* ("Onderdeel | Vlak | Rand"). Part is the default and today's behaviour. In Face and Edge mode, hovering highlights; a click selects; Ctrl-click adds (up to two faces, to show their relation); Esc goes back to Part mode. The mode is greyed out when the model has no B-rep (other formats, STEP with only tessellation).
- **Edge pick.** Among the boundary edges of the face under the cursor, the nearest edge within `SNAP_PX` on screen wins. That is the face's own loops, so the pick does not scan the model.
- **Properties panel, part selected:** sections *Geometry* (faces by type, plane groups), *Holes (n)* (grouped rows, a click selects their faces) and *Sheet metal* (yes or no with reason; thickness; bends; K with rule *Fixed / DIN 6935* and value field; flat size; *Flat pattern* toggle; *Bend table*; *Export DXF…*).
- **Panel, face or edge selected:** sections *Face* or *Edge* as in *Face properties*. With two faces, *Between faces*.
- **Bend table** opens under the 3D view, using the schedules table component. Rows select the bend faces.
- **Flat pattern placement.** It lies in the plane of the base flange's A face, moved within that plane along the flat `x` axis by the part's extent along `x` plus 10 % of the part size, so it sits beside the part and does not overlap it. Fill colour is the part colour lightened, both sides lit. An overlapping pattern is drawn in the warning colour.
- **Settings.** K-factor default (0.44) and rule, next to the unit settings. Sheet metal values show at least two decimals in mm, whatever the length decimals setting (default 0).
- **Labels** are bilingual, as the rest of the app.

## Export: DXF flat pattern (`brep::dxf`, used by the app and MCP)

- DXF R12 ASCII (`$ACADVER AC1009`), millimetres (`$INSUNITS 4`, `$MEASUREMENT 1`). Entities: LINE, ARC, CIRCLE, TEXT. No polylines, so every reader takes it.
- Layers: `OUTLINE` (colour 7), `HOLES` (colour 7; circles and cutouts), `BEND_UP` (colour 5), `BEND_DOWN` (colour 1), `BEND_NOTES` (colour 8; TEXT such as "UP 90.0° R2.00 BA 4.52" at the middle of each bend line). Only centre lines go on the bend layers. Tangent lines are not exported.
- Coordinates: the flat frame, origin at the bounding-box minimum, A side up.
- Written through `export::write_file` (atomic `.part` and rename). In the browser it is a download. It is not a model `Writer` in the Save as dialog, because it exports one part, not the model.

## MCP tools

Names follow the existing ones. Element ids are passed as `get_element` takes them. Positions are scene coordinates in metres (as `get_element` bounds). Sizes are in millimetres with a `_mm` suffix.

| Tool | Arguments | Answer |
|---|---|---|
| `get_faces` | `element`, `kind?` (plane, cylinder, cone, sphere, torus, other), `offset`, `limit` (default 100, at most 1000) | `total`, `truncated`, faces: `index`, `kind`, `area_mm2`, `centroid`, `normal` or `axis` + `radius_mm`, `convex`, `feature` |
| `get_holes` | `element`, `"selection"` or `"model"`, `offset`, `limit` | per part: holes with `diameter_mm`, `depth_mm`, `kind` (through, blind, other), `bottom` (none, flat, drill_point), `tip_angle_deg`, `stepped`, `axis {point, direction}`, `faces`; and `groups` (`count × Ø`) |
| `get_sheet_metal` | `element`, `k_factor?`, `k_rule?` (fixed, din6935), `base_face?` | `sheet_metal`, `reason`, `thickness_mm`, `base_face`, `bends[]` (`index`, `angle_deg`, `included_deg`, `inner_radius_mm`, `direction`, `length_mm`, `k_factor`, `bend_allowance_mm`, `bend_deduction_mm`, `line` in flat mm), `flat_size_mm`, `flat_area_mm2`, `checks` |
| `unfold` | `element`, `k_factor?`, `k_rule?`, `base_face?`, `show` (default true) | as `get_sheet_metal`, plus `outline_edges`, `holes`, `cutouts`; shows or hides the flat pattern in the viewer |
| `export_flat_pattern` | `element`, `path?` (default `<file>-<element>-flat.dxf` next to the model), `k_factor?`, `k_rule?`, `base_face?`, `overwrite?` | path written, layers and entity counts; `destructiveHint: true` as `save_file` |

`pick` gets `face` and `edge` (index and a short summary) in its answer when the element has a B-rep. `get_element` gets a `brep` summary (face count by kind, hole count, `sheet_metal`). `INSTRUCTIONS` gets one sentence on face picking and sheet metal.

## Performance budget

- **Camera frames: zero added work.** No added uniforms, no rebuilds. The overlay buffers change only on selection, hover or toggle.
- **Load:** capturing the summary adds at most 10 % to STEP load time and 20 % to its peak memory. This is measured with the existing ignored `step_folder` test (`O3V_STEP_DIR`) on the real samples, before and after.
- **Hover in Face mode:** the existing ray pick plus one binary search over face runs. The highlight overlay is rebuilt only when the hovered face changes, and is capped at 20 000 triangles; larger faces show their outline only.
- **Analysis per unique body:** recognition under 5 ms per 1 000 faces, and detection plus unfold under 20 ms per 500 faces (release build, reference PC). This is checked in a test with a generous upper bound and printed by the folder test.
- **Background pass** (desktop): one worker thread, at most 2 000 bodies or 2 s, then on demand. It never blocks the UI thread.
- **Browser:** the selected part only, after the next frame. A body over 5 000 faces is analysed only on an explicit *Analyse* click.

## Web support

`src/brep/` is pure Rust and compiles for wasm32 (CI already builds wasm32). STEP opens in the browser today, so face picking, holes, sheet metal and the flat pattern work there. *Export DXF* downloads through `web::download`. MCP is desktop-only, as today.

## Limits (stated in README and panel help)

- Only cylindrical bends with straight bend lines. Conical bends, rolled sheets, tubes and hems are flagged, not unfolded.
- No forming features. Parts with louvres, embosses, ribs or formed holes fail check C3, with the faces listed.
- Non-developable faces on the A side refuse the unfold, and the faces are named.
- One K-factor per part. Per-bend K, gauge tables and bend tables from the shop are follow-ups.
- Analysis per body. A multi-body part is analysed body by body.
- STEP parts that have only tessellation (AP242 meshes) have no B-rep, so there is no face picking or recognition.
- Inch files: the IR is in millimetres, so values are right. Display follows the unit settings, and DXF is always in millimetres.

## Test plan

### Fixtures (self-made, in `src/brep/tests.rs`)

The STEP writer in the `step.rs` tests (`Step`, lines 431-626) moves into a shared `#[cfg(test)]` module and gets two generators:

- **`extruded_profile(profile, width, holes)`:** a closed 2D profile of lines and arcs, extruded by `width`. The caps become planar faces with the profile as their loop. Line segments become planar side faces, and arcs become cylindrical faces (concave or convex by the arc's turn). Optional holes are through cylinders in a named planar face, split into two half faces at a seam as many writers do. This gives L, U, Z and hat sections with any radii and angles.
- **`box_with_flanges(w, d, h, t, r)`:** a base plate with four flanges bent up at 90° and open corners. Each flange and its bend is one extruded L profile along one side of the base, sharing the base's A and B face edges. The corner has a vertical edge of length `t` shared by two bend end faces.

Fixtures and expected results:

| Fixture | Expected |
|---|---|
| L-bracket `A=50, B=30, t=2, r=2, w=40, θ=90°` | sheet metal; `t = 2.000`; 1 bend, 90°, R 2, up; flat `76.524 × 40.000 mm` (±0.005); BA 4.524, BD 3.476; DXF: 4 LINE on OUTLINE, 1 LINE on BEND_UP of length 40 at `x = 46 + BA/2` |
| L-bracket at 120° deflection and at 45° | flat length from `(A − OSSB) + (B − OSSB) + BA` with `tan(θ/2)` |
| U-channel, 2 bends same direction | 2 up bends; flat = sum of straight parts + 2 BA |
| Z-section | 1 up, 1 down |
| Box with 4 flanges: base A face `100 × 60`, flange height `h = 20` (outer), `t = 1.5`, `r = 1.5` | 4 bends, all up; cross-shaped outline of 12 lines; flat size `(100 + 2·(BA + h − r − t)) × (60 + 2·(BA + h − r − t))`; no overlap |
| Box with flanges longer than half the base | *overlap* reported |
| L-bracket with two Ø 6.6 holes per flange | 4 holes through, depth 2; 4 CIRCLE on HOLES. A hole in flange 2 whose centre is at distance `c` from the outer face of flange 1 (measured along flange 2) lands at `x = (A − r − t) + BA + (c − r − t)` in the flat |
| Plate 100 × 60 × 10 with holes | 2 × Ø 8.5 through (split halves), 1 × Ø 6 blind depth 6 with flat bottom, 1 × Ø 5 blind depth 8 with 118° drill point (tip depth `2.5 / tan 59° = 1.502`); not sheet metal (C3: the blind holes' walls and bottoms are not edge faces). The same plate without the blind holes is a flat blank (C2: `10 ≤ 0.2·√5887 = 15.3`) |
| Block 100 × 50 × 20, the cylinder pin | not sheet metal (C2), reason text |
| Bracket of two thicknesses (3 and 5 mm) | not sheet metal (C3), reason names 3.00 and 5.00 |
| Thick bent part `t = 20`, legs 60 | not sheet metal (C2) |
| Fillet block: a block with one concave R5 fillet and one convex R3 round | 1 fillet R5, 1 round R3, no holes |

Unit tests of the maths: BA, BD, OSSB and DIN 6935 `K` values (`r/t = 1 → K = 0.325`, `r/t = 5 → 0.5`). The unfold transform is tested on the 2D example (`u = z`, `e_F = −y`, `e_G = +x` gives `α = −90°`, `d_F = +x`). Round trip: every bend's map at `ψ = θ` equals `T_G`.

### Viewer and MCP tests

- Face runs survive `split_layer` (forced small limits) and `Model::combine`, checked with `Model::face_at` on known triangles.
- The memory cap: with a tiny cap, `dropped > 0` and face pick falls back.
- Remote tests (`src/remote.rs` test harness `run(&mut app, …)`): `pick` returns `face.kind = "plane"` and the normal on the L-bracket; `get_faces`, `get_holes`, `get_sheet_metal` and `unfold` return the values above; `export_flat_pattern` writes a DXF into a temp dir, refuses to overwrite without `overwrite`, and the DXF **re-opens with the viewer's own DXF loader** (desktop), with layers OUTLINE, HOLES and BEND_UP and the expected extents.
- The overlay `Arc` does not change across 100 simulated camera frames (no per-frame rebuild).
- `cargo check --target wasm32-unknown-unknown`.

### Real samples (local verification only)

Download sheet metal and machined-part STEP samples whose terms allow it to `C:\Users\AIA12\AppData\Local\Temp\step-samples`. None are committed. A new ignored test `sheet_metal_folder` (alongside `step_folder`, using `O3V_STEP_DIR`) prints per part: sheet metal yes or no with reason, `t`, bends, flat size, refusals, overlap, and analysis time. Before merge, the PR description records the result per sample:
- known sheet parts are detected (and the reason for any miss);
- no machined part is detected;
- flat sizes agree with dimensions printed on the sample's drawing or description, where one exists;
- load time and memory are within budget.

## Calls made in writing this design

- **Summary instead of keeping `CadIr`.** It is bounded by the mesh size, and it is also what FCStd, BREP and IGES produce.
- **Face runs instead of a per-triangle face id.** 8 bytes per placed face instead of 4 bytes per triangle, and it survives `split_layer`.
- **Outward direction from the tessellator's oriented normals**, not from `sense` bookkeeping.
- **A side found by smooth connectivity**, so splits, seams and coplanar faces need no special cases.
- **R12 DXF with LINE, ARC and CIRCLE**, hand-written (about 200 lines), for the widest CAM compatibility and no dependency.
- **Bend deduction reported in the mold-line convention**, with only BA used for the geometry, so the two conventions cannot disagree with the pattern.

## Scope of the first PR (this branch)

In:
1. `src/brep/` with capture in the STEP loader, face runs, the memory cap and `Model.brep`.
2. Face and edge pick mode, the GPU overlay (highlight, edge, hole axes, flat patterns) and the panel sections Face, Edge and Between faces.
3. Recognition: planar faces and plane groups, holes (through, blind with flat or drill-point bottom, `stepped` flag), fillets and rounds (cylinder and torus), and the holes list per part.
4. Sheet metal: detection (C1–C4), unfold with the tree, cycle check and refusals, the bend table, flat size, the K setting (fixed and DIN 6935, per-part override), the flat pattern toggle and DXF export.
5. MCP: `get_faces`, `get_holes`, `get_sheet_metal`, `unfold`, `export_flat_pattern`; `face` and `edge` in `pick`; `brep` in `get_element`.
6. Tests above, README.md and README.nl.md (STEP section, MCP list, limits).

Out (follow-ups):
- **F1 Measure integration:** snapping to cylinder axes, circle centres and face centroids (`SnapKind::Axis`, `SnapKind::Centre`, `Entity::Axis`), exact face-to-face distance and angle in the measure tool, hole centre distances; MCP `measure_faces`.
- **F2 Holes:** counterbore, countersink, slots and obrounds, hole tables to CSV, cosmetic threads from AP242 PMI.
- **F3 Sheet metal:** hems and teardrops, conical bends, rolled sheets with a seam, bends without a flange between, per-bend K, gauge and bend tables, minimal bounding rectangle, dashed bend lines in the viewer.
- **F4 Forming features** marked (not unfolded) in the flat pattern.
- **F5 Large models:** re-decoding dropped bodies on demand from the file (desktop), persisting the K setting.
- **F6 Other formats:** FCStd, BREP and IGES loaders call `brep::capture` (branch `feat/freecad-formats`).
- **F7 Flat pattern as SVG** and in a printable sheet.

## Risks

- **Writers that split faces oddly** (seams, coplanar splits, tiny sliver faces). Grouping by coaxiality and coplanarity, and the A side by smooth edges, absorb most of this. The real-sample run will show the rest.
- **Tolerance of real files** (gaps of 1e-4 to 1e-2 mm between faces). ε scales with the model, and thresholds are named and tuned.
- **Panel complexity.** The new sections appear only when they have content. Face mode is off by default.
- **Edge cases of `split_layer`** for face runs. Covered by a forced-split test.

## Open questions

- Should Face mode be on a modifier key (Alt-click) as well as the ribbon switch? The proposal is the switch only for now.
- Should the default K rule be DIN 6935 for users with Dutch or German settings? The proposal is fixed `K = 0.44` everywhere, as requested.
