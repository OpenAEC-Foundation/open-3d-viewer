# The Modeller: roadmap of the next sub-projects

**Goal:** make the Modeller work as a modeller: a parametric BIM modeller's elements, levels and
views, with a mesh modeller's edit mode on top, following Open BIM Studio where it has an answer.

**Reference:** Open BIM Studio, `main` on GitHub (`OpenAEC-Foundation/open-bim-studio`). Read the
latest `main` before each sub-project; a local clone can be hundreds of commits behind. Its
`docs/wiki/User-guide.md` and `docs/images/screenshots/` give the overview, the rules are in the
`.dl` sources named per sub-project below.

**How this document is used:** it fixes the order, the scope and the decisions of each
sub-project. Each sub-project gets its own task-level plan in this folder when it starts, and
ends in a commit series on its own branch and a PR. Nothing here is built yet.

## Constraints for every sub-project

- Modeller only: commands, panes and behaviour show in the Modeller product, not in the Viewer.
- Code apart: new modules under `src/app/modeller/`, kernel work in `crates/bim-core`. Shared
  code gets short calls into those modules, no modelling logic.
- One engine: views, sections, sheets, picking and rendering are the Viewer's; the Modeller adds
  to them and does not fork them.
- Everything the Modeller stores is IFCX; what it writes it reads back exactly.
- Every change of the project is one undo step (Ctrl+Z, Ctrl+Y).
- A refusal of the kernel leaves the project as it was and says why.
- Every interface text in every language of `locales/`.
- No names of other commercial products in code, comments, documents or commit messages.

## What exists (PR #161)

Walls, slabs, columns, beams, systems and objects with types; doors and windows in walls and
timber frames; connections between linear elements and between framed systems; move, copy,
rotate, mirror; undo; grips on walls; edit mode (Tab) on the points of an element; levels that
can be renamed and moved; grid lines; a project browser; taking over a wall of an opened file.

## Order

The order follows what depends on what: selection and keys first, since every later tool lives
in that interaction; then the data the elements need (levels on walls, types with layers); then
the editing built on both.

| # | Sub-project | Delivers |
| --- | --- | --- |
| 1 | Select tool and shortcuts | A Select mode, box selection, the keys of Open BIM Studio |
| 2 | Panes as in Open BIM Studio | Project browser left, model explorer above the properties right |
| 3 | Walls between levels | A wall has a base level with offset and a top: free, a level, or what lies above |
| 4 | Edit mode for linear elements | Tab on a beam or column: its ends, grab, axis lock, typed distance |
| 5 | Boundary sketch | Tab on a slab: its outline as a sketch with lines and arcs |
| 6 | Types with layers and materials | A taken-over wall keeps its layers and materials, in IFCX and IFC4 |
| 7 | INB library | The types of the INB template as a catalogue of the library |
| 8 | Free-standing doors and windows | A door or window is an element of its own that cuts what it stands in |
| 9 | Dimensions that follow and drive | References to elements, temporary dimensions, a typed value moves the element |

Sub-projects 1 and 2 are interface only. 3, 6 and 8 change the kernel's data and the file, so
each carries a reader for files written before it.

## 1. Select tool and shortcuts

**Open BIM Studio:** only the Select tool selects (`ui/shell/main.dl`, box selection around line
1820); Esc rolls back a move, returns to Select and clears the selection
(`ui/editor/bim_editor.dl`, 1697-1734); a box dragged to the right takes what lies inside it, to
the left also what it touches, Shift adds (`bim_editor.dl`, 473-525); single keys and two-letter
commands within 750 ms (`bim_editor.dl`, 964-1205, 1345-2400).

**Scope:**
- A Select command on the Model tab, on by default in the Modeller; the status line names the
  tool and its key.
- While another tool is on, a click belongs to that tool. Esc: drop the step under way, else go
  to Select, else clear the selection.
- After a tool: Move, Copy, Rotate, Mirror and Grid return to Select; Wall, Beam and System
  chain until Esc; Door and Window stay on.
- Box selection in the Modeller: to the right a window, to the left a crossing, Shift adds.
- Single keys: S select, W wall, G grab, I grid, D door, O window, C section, Del delete,
  PgUp / PgDn the level above or below. Two letters within 750 ms: MV move, CO copy, RO rotate,
  MM mirror, CL column, BM beam, DI dimension. The command list shows each key.

**Decisions:** undo stays Ctrl+Z and redo Ctrl+Y (Open BIM Studio uses Z and Y alone). Keys act
only while the Modeller is shown and no text field has the focus. The Viewer keeps selecting as
it does now.

**Done when:** a test drives each key and each Esc step; a box to the left and one to the right
select what the rule says; the Viewer's selection tests pass unchanged.

## 2. Panes as in Open BIM Studio

**Open BIM Studio:** the project browser on the left (`ui/panels/bim_ui.dl`, 2443-2690); the model
explorer above the properties on the right, a tree of Project, Storeys (lowest first) with their
categories and elements, Grids, Types, Views and sheets (`ui/panels/bim_model_explorer.dl`,
252-356). A click on a branch opens it, a click on an element selects it.

**Scope:**
- The project browser of PR #161 gets Open BIM Studio's groups: floor plans, 3D views,
  elevations, sections, schedules, sheets; then building components with their count.
- A model explorer in the right pane above the properties, in the Modeller: Project, Storeys
  (each level, its categories, their elements by name), Grid lines, Types, Views and sheets. A
  branch lists at most 400 rows.
- A click on an element selects it and the properties show it; the selected element is marked
  in the tree.

**Decision:** the Tree / Graph switch of Open BIM Studio is left out; the tree alone.

**Done when:** the tree lists the demo building per level and category; a click selects; the
Viewer's right pane is as it was.

## 3. Walls between levels

**Open BIM Studio:** a wall has `levelId`, `baseOffset`, `height` and `topLevelId`
(`elements/planar/records.dl`, 17-45); its top is free, up to a level, or attached to what lies
above; every committed edit refreshes the tops, and a typed height frees the top
(`bim/bim_model.dl`, 2379-2391, 2512-2560).

**Scope:**
- A wall, a column and a standing system keep a base level, a base offset, and a top: a height,
  or a level with a top offset. The kernel works their base and height out from the levels.
- Moving a level moves what stands on it and stretches what reaches up to it, in one step.
- Properties: Base level, Base offset, Top (Free / the levels above), Top offset; Height is
  read-only while the top is a level, and typing one frees the top.
- The Wall tool places from the level of the plan shown to the level above it, when there is one.
- The top grip (PR #161) frees the top, or takes the level it is dropped on.

**Decisions:** attaching a top to a slab or roof waits for roofs. Levels move into the
project's history, so undo takes back a level's move with its elements (now it does not).

**Done when:** raising a level by 0.4 m raises the walls on it and lengthens those below that
reach it; a file written before this opens with the same geometry.

## 4. Edit mode for linear elements

**Open BIM Studio:** Tab on a beam or column shows its two ends in 3D; G grabs, X / Y / Z lock an
axis (the same key again frees it), digits type a distance, Enter places, Esc cancels
(`ui/editor/bim_editor_core.dl`, 7922-8044, 8615-8659). Profile and size change through the type.

**Scope:**
- Tab on a selected beam, column or member opens edit mode on its ends, also for one taken over
  from an opened file (see below).
- In a grab: digits, a point or comma and Enter move the picked end that far along the locked
  axis, or along the member when none is locked.
- The properties in edit mode: start and end (x, y, z), length, roll, type.
- Taking over a beam or column of an opened file: its axis and its profile where the file
  defines them, as walls are taken over now.

**Decision:** what a file's beam needs to be taken over (a straight axis and a profile the
library has or can be given) is settled in this sub-project's plan, after reading how the
loader knows beams.

**Done when:** Tab, G, Z, `0.5`, Enter lifts a beam's end by 0.5 m in one undo step.

## 5. Boundary sketch

**Open BIM Studio:** Tab on a floor, roof or ceiling opens its outline as a sketch
(`ui/editor/bim_boundary_sketch.dl`): L line, A arc through three points, R rectangle, T trim,
G moves a point or an edge, Del removes segments, Tab finishes as one step, Esc leaves it
unchanged. An arc is a segment with a bulge, the distance from the chord's middle to the arc's;
the element keeps a polygon and which edges each arc spans, so the arcs come back. The outline
is one closed loop; holes are separate openings.

**Scope:**
- The kernel's slab outline takes segments with a bulge; it is tessellated by chord tolerance
  and written with its arcs.
- Tab on a slab opens the sketch on its level: move a corner or an edge, add a corner on an
  edge, remove one, draw lines and three-point arcs, a rectangle.
- A sketch that is not one closed loop, or crosses itself, cannot be finished and says where.
- Floor openings: a rectangle between two clicks on a selected slab.

**Decisions:** radius and centre arcs, tangent arcs, circles, ellipses and trim come later.
Walls and systems keep their rectangle; their elevation contour is not in this sub-project.

**Done when:** a slab with one arc edge is drawn, saved, opened and edited again with the arc
still an arc.

## 6. Types with layers and materials

**Open BIM Studio:** a wall type is a name, a role and layers of material and thickness; each
layer has a function and a core flag (`ui/properties/bim_type_layers.dl`). On IFC import the
layers are read and a type with the same layers is reused (`ifc/bim_ifc_native.dl`, 191-255,
673-688); on export a material layer set per type and its usage per wall (`ifc/bim_ifc.dl`,
672-700, 828).

**Scope:**
- The reader of opened files gives a wall's layers: material name, thickness, function where
  the file has it, and the material's colour and class.
- Taking over a wall gives it a build-up with those layers, a type of the project's library
  made for it or reused when the layers are the same, and the file's own properties.
- IFCX and IFC4 written from the project carry the layers and materials of each wall and slab.

**Decision:** material properties beyond name, colour and class (thermal, structural) are
copied as properties of the material where the file has them, not interpreted.

**Done when:** a layered wall of a test file is taken over, exported as IFC4 and read back with
the same layers, thicknesses and material names.

## 7. INB library

**Open BIM Studio:** the types of the INB template (an IFC file of the OpenAEC Foundation) are
extracted to a catalogue at build time (`tools/inb_type_catalogue.py`,
`localisation/nl/types/catalogue.json`): 39 walls, 33 slabs and roofs, 7 columns, 7 beams.

**Scope:** the catalogue as a library the Modeller can add to a project (the Library tab), its
types under their own group, placed with the Wall, Slab, Column and Beam tools.

**Decision:** the catalogue is read from the JSON, shipped with the app; no script at run time.
Depends on 6 for layered types.

**Done when:** a wall of an INB type is placed and its layers show in the properties.

## 8. Free-standing doors and windows

**Open BIM Studio differs:** there a door or window is always hosted in a wall or a system
(`architectural/openings/records.dl`). The wish here is a door that is not tied to a wall.

**Scope:**
- A door or window is an element of its own: a place, a direction, a type (width, height, sill).
- It cuts an opening in every wall or system it stands in; moved out of a wall it stays, and the
  wall closes.
- The Door and Window tools still aim at a wall and align to it; a click away from any wall
  places one free.
- Openings of files written before this become such elements when read.

**Decision:** deleting a wall leaves its doors and windows standing.

**Done when:** a door is moved from one wall into another: the first closes, the second opens.

## 9. Dimensions that follow and drive

**Open BIM Studio:** a dimension keeps two references, each an element and a feature of it (a
wall's start, end or face, an opening's side, a grid line), resolved when drawn, so it follows
(`views/drawings/records.dl`, 36-60). It has no temporary dimensions and no dimension that moves
an element; the wish here goes further.

**Scope:**
- A dimension between two picked features of modelled elements and grid lines; chains.
- It follows its elements; a reference whose element is gone is marked.
- A selected element shows temporary dimensions to its neighbours and grid lines; typing a
  value moves the element.
- Typing a value in a dimension between two elements moves the one picked last.

**Decision:** which element moves, and what a locked dimension would mean, is settled in this
sub-project's plan.

**Done when:** a wall 3.00 m from a grid line is put at 3.60 m by typing in its temporary
dimension, in one undo step.

## Open questions

1. Sub-project 8 departs from Open BIM Studio. Is a door that stays when its wall is deleted
   what is meant, or a door that can stand free but goes with a wall it was placed in?
2. Sub-project 9: is "the element picked last moves" the rule wanted, or the one that is not
   pinned?
3. PR #161 has grown large. Merge it first and start sub-project 1 on a new branch?
