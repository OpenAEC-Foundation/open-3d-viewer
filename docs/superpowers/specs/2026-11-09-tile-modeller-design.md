# The tile modeller: a kernel in tile frames and five base elements

**Status:** first slice, 2026-11-09. Builds on the levels, views and IFC tab work (#105, #106, #109).

## Intent

Open 3D Viewer Studio grows into a BIM modeller. Viewer, modeller and validator share one code base.
The [phased design of 2026-11-06](2026-11-06-bim-modeller-design.md) put tiles in phase 5 and
modelling in phase 6. The project owner changed that order:

1. **First the kernel.** It must handle the geometry: exact where it decides, in f64 tile frames,
   refusing rather than answering wrongly. This matters most.
2. **Then the five base elements.** All intelligent BIM objects are built from five base elements,
   as in Open BIM Studio:
   - **point**: a type placed at one point;
   - **linear**: a profile along a line;
   - **planar**: a wall or a slab, with openings;
   - **system**: a recipe on a domain, generating members and panels;
   - **connection**: where elements meet.

The requirements, anti-goals and constraints of the 2026-11-06 design stay in force. Its phases 0
to 2 (frame budget, selection, direct manipulation of meshes) are not a condition for this work.

## What the kernel takes over from Open BIM Studio

Open BIM Studio's kernel and decision records are the reference. Their architecture is followed;
the DynLex code is not ported.

| Rule | Source in Open BIM Studio | Here |
|---|---|---|
| Kernel coordinates are f64 in a local tile frame; f32 only for the GPU | ADR-0006 | `bim-core::tile`; the app converts tile-local f64 to display f32 |
| A fixed square tile grid anchored at the project origin; an element belongs to the tile of its placement origin, may overhang it, and is never split | ADR-0016 (proposed) | `TileGrid`, `TileKey`, `Element::tile` |
| Tile origins sit on an exact grid | project origin in integer millimetres | tile origins in integer millimetres, so moving between tiles is exact in f64 |
| References between elements go by stable element id, never by position | ADR-0011, ADR-0016 | `ElementId(u64)`, never reused |
| Topological decisions use exact predicates | ADR-0006 | `orient2d` / `orient3d` from the `robust` crate (Shewchuk's adaptive predicates) |
| Refuse with a typed reason, never return a wrong answer | boolean findings, R-16 | `Refusal` on every constructor and evaluation |
| Planar first: walls with openings are built directly, without booleans | `kernel/fast/` | a wall is the extrusion of its elevation with the openings as holes |
| Geometry is derived from definitions; derived data never changes the model | ADR-0013 | elements hold definitions; solids and meshes are evaluated from them |
| The five base element classes | `elements/{object,linear,planar,connection}`, `pattern/` | `bim-core::element` |
| IfcX is the reference schema, IFC4.3 the vocabulary | ADR-0010 | IFC class names per element; the definition is saved in IfcX |

## The first slice

### `crates/bim-core` (headless)

- `tile`: `TileGrid { size_mm }` (1 km by default), `TileKey { x, y }` and conversion between
  project coordinates and tile-local points. Tiles are a horizontal grid; heights are not tiled.
- `predicates`: exact `orient2d` and `orient3d`, and segment crossing built on them.
- `profile`: 2D profiles (rectangle, circle, I-section, polygon with holes), checked: simple,
  non-degenerate, holes inside, orientation normalised.
- `solid`: a closed planar polyhedron in tile-local f64. Two constructions: a prism of a profile,
  and a profile swept between two arbitrary planes (needed for joins). Each checks itself: every
  directed edge has exactly one reverse twin, and the volume is positive.
- `mesh`: triangles of a solid, with face holes, for display, in the tile frame.
- `element`: the five base elements and their evaluation to solids.
- `model`: elements by id, the tile index, and evaluation with dependencies (connections change
  the ends of the members they join).

### The five base elements in the first slice

| Element | Definition | IFC class | Evaluates to |
|---|---|---|---|
| Point | position, rotation, size | `IfcBuildingElementProxy` | a box |
| Linear | start, end, profile, roll | `IfcBeam`, `IfcColumn` or `IfcMember` by direction | the profile swept between its end planes |
| Planar, wall | axis start and end, base, height, thickness, openings | `IfcWall` | its elevation with holes, extruded through the thickness |
| Planar, slab | outline, top, thickness | `IfcSlab` | the outline extruded downwards |
| System | a vertical domain by its line, spacing, member profile, panel thickness | `IfcCurtainWall` | mullions and transoms (linear parts) and panels (planar parts) |
| Connection | primary and secondary member, butt or mitre | `IfcRelConnectsElements` | no body of its own: it moves the end planes of its members |

### In the app

- A **Model** tab in the ribbon with **New model** and one tool per base element: Point, Beam,
  Column, Wall, Slab, System. **Connect** and **Mitre** join two selected linear elements.
- Points are clicked on the work plane: the level of the floor plan shown, else the level nearest
  0. Clicks snap to element ends within 12 px, straighten within 4° of the first point's axes and
  round to 10 mm; walls, beams and systems continue from the last end.
- Modelled elements show as a model of their own, on layers per IFC class. They are selected,
  hidden and measured like any element, and Delete removes them.
- Saving as IFCX writes each modelled element with its IFC class, its mesh and its definition
  (`openaec::element`, with the tile). A connection has no body and so no node: its definition
  travels with its primary member. Reading the file back gives the definitions back instead of
  the meshes, as editable elements.

## Languages

The interface speaks Dutch and English, through `Language::text(nl, en)` in the vendored
`openaec-ui`. German and French are added:

- `Language` gets `German` and `French`; `text(nl, en)` looks up the English text in a catalogue of
  that language and falls back to English.
- The catalogues are `locales/de.json` and `locales/fr.json`, keyed by the English text, so the
  312 call sites stay as they are. Translators edit the JSON files.
- `--language` and `OPENAEC_TAAL` accept `de` and `fr`; numbers use the decimal comma in German
  and French.
- The change to the vendored crate is recorded in `vendor/UPSTREAM.md`; the Style Repo is the
  source of truth and gets the same change upstream.

## Calls made in writing this design

The author made these calls, not the project owner. They are open to correction:

- The branch builds on #109 (stacked), because the modeller needs its levels and IfcX document.
- The crate keeps the working name `bim-core` of the 2026-11-06 design.
- Tile size 1 km and a horizontal (x, y) grid.
- The exact predicates come from the `robust` crate (MIT or Apache-2.0), already in the
  dependency tree, rather than a port of our own.
- A system's domain in the first slice is a vertical rectangle given by its line, not the face of
  a host element.
- A butt connection cuts the secondary member with the plane of the primary member's face; a mitre
  cuts both members with the bisecting plane.
- The whole model is evaluated again after each edit, and the modelled model is uploaded again.
  The edit layer of the 2026-11-06 design is the follow-up for large models.
- German and French are the added languages. Their catalogues are a first translation that needs
  review by native speakers.
- A test fails when an interface text (`text(nl, en)` or `translate(en)` with literal strings)
  has no German or French translation, so new texts arrive translated.
- Property names of modelled elements (Length, Thickness, Tile) are data, like an IFC file's, and
  stay English in every interface language.

## Not in this slice

Undo of modelling steps; handles and typed parameters; snapping beyond element end points; types
and layer sets; openings by a tool; curved geometry and general booleans; paging the document by
tile (R-14 spike).

## Second slice: booleans (2026-11-09)

The project owner chose booleans as the next kernel step, before wall corners and curved geometry.

- `bim-core::boolean`: union, difference and intersection of closed planar solids.
  - The faces are cut into convex polygons and sorted in binary space partitioning trees, the
    classic polygon CSG.
  - Every side decision is an exact `orient3d` against three input points that span the plane. A
    polygon split off another keeps its parent's plane, so no decision depends on a constructed
    point.
  - Crossing points are constructed in f64 and rounded once.
- **Putting the result back together.** Points within the linear tolerance (1e-6 m) are welded.
  A point on another polygon's edge splits that edge, so no T-junctions remain. The result must
  pass the closed-solid check. Otherwise the boolean is refused (`BooleanFailed`), and a result
  that would be empty is refused as `Empty`.
- **The cut connection** (`ConnectionKind::Cut`). The primary element cuts its body out of the
  secondary one, for example a beam through a wall or a duct through a slab.
  - It works for any two elements with a body.
  - A cutter cuts with its body before cuts (with its joints), so cuts never depend on each other:
    two elements may cut each other.
  - A cut that does not reach its target is refused (`NotTouching`).
  - A part of a system that a cut takes wholly goes.
- In the app, **Cut** in the Connection group: select the cutting element first, then the one
  it cuts.
- Calls made: the BSP method rather than a mesh arrangement (simpler, and exact where it
  decides). The result faces are convex polygons, not merged back into the original faces.
  Cutters stay visible; an invisible opening body is a follow-up.

## Third slice: edit mode (2026-11-09)

The project owner asked for an edit mode like Revit's and Blender's, with Tab, taking ideas
from Open BIM Studio (its edit mode, grips and boundary sketch).

- **Opening it.** Tab on one selected modelled element opens its edit mode; Tab leaves it. Without
  such a selection, Tab moves the focus as before.
- **Points.** The mode shows the points of the element's definition: a member's ends, a wall's or
  curtain wall's axis ends, a slab's corners, an object's place. They are drawn over the view
  with the lines between them.
- **Moving.** Drag a point to move it, and the other picked points with it. Click picks a point
  and Ctrl adds one.
  - G grabs the picked points: they follow the cursor and a click puts them down.
  - X, Y and Z hold the move to an axis. Up or down, a planar element or a system moves whole,
    since it has one height.
  - A picks every point, and Delete removes a slab's picked corners (three stay).
  - Esc drops a move under way, else leaves.
- **Snapping.** A move snaps to the points of the other modelled elements and is rounded to
  10 mm.
- **The kernel.** The change is evaluated when the move is put down; a refusal leaves the element
  as it was and shows why.
- **Calls made:**
  - Dragging is done in an overlay over the viewport (as the section box's handles are), so a
    drag on a point does not orbit the camera.
  - During a move the overlay shows the points and lines, not the evaluated solid; the solid
    follows when the move is put down.
  - G, X, Y, Z and A are edit mode's keys while it is open, and the grid shortcut G waits until
    it closes.
- **Next:** typed parameters (thickness, height, profile, openings) in the properties pane, a
  live solid during a move, and a sketch mode for outlines with arcs.

## Fourth slice: the definition typed in (2026-11-09)

- The right pane gets a Definition section for one selected modelled element. It holds the
  element's name and the values of its definition, each typed in:
  - a wall's height, thickness and base;
  - a slab's thickness and top;
  - a member's rectangular profile or diameter, and its roll;
  - an object's size, base and turn;
  - a curtain wall's height, base, spacing, member size and panel thickness.
- A wall's openings are listed with their distance from the start, sill, width and height.
  **Add door** and **Add window** put one in the middle of the wall; **Remove** takes one out.
- A value is used as soon as it is valid and the kernel can make the element with it. Until then
  the field keeps what was typed and the banner says why. Enter shows the value in use again, as
  the View section does.

## Fifth slice: wall corners (2026-11-10)

- **Corners.** Walls whose axis ends meet (within 1 mm) and whose heights overlap get a corner.
  Both are cut along the vertical plane through their inner and their outer corner, where their
  faces cross. For one thickness that is the mitre; for two thicknesses the walls still meet face
  to face. It does not matter which way either wall was drawn.
- **T-junctions.** A wall whose end lies on another wall's axis, away from its ends and within its
  half thickness, stops at that wall's face; the other runs through.
- **Left as they are:**
  - three or more walls at one point;
  - walls at heights that do not overlap;
  - corners so sharp that the cut would reach more than eight wall thicknesses from the joint.
- **How it is made.** A joined wall is made with its ends reaching past its corners. It is then cut
  back by a boolean intersection with its footprint in plan, a prism through its whole height,
  layer by layer, so a layered wall's layers meet along the same cut.
- **Fallback.** When the cut cannot be made, the wall is made as it is.
- **Calls made:**
  - Joins are found from the walls' definitions every time a wall is evaluated (no join elements
    to keep up to date). Moving a wall in edit mode re-cuts its neighbours.
  - Cross junctions and joins with slabs or roofs are follow-ups.
