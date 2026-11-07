# Views, sheets and dimensions

**Status:** built in small steps on 2026-11-07, each shown to the project owner in the running app; follows [levels](2026-11-07-levels-design.md).

## Intent

After levels, the project owner wants to make drawings in the viewer: views of the model (floor plans per level first), sheets with those views placed on them, views moved over a sheet, and dimensions. Within one project there are several views, each in its own tab; selecting in one view selects in all of them. Everything lives in the IfcX project document, which will be saved as `.ifcx` later.

## What exists

- **Floor plans** (`document::PlanView`): made per level in the explorer's **Views** tab, named after the level (`<level> (2)` for a second one). A plan looks from 0.3 m under its level up to a cut 1.2 m above it. A plan goes with its level.
- **View tabs** (`app::DocTab`): the pinned 3D view, then the open floor plans, schedules, Revit sheets and document sheets. Each floor plan tab keeps its own camera and section box (`ViewSlot`), restored when its tab shows again; the 3D view keeps its own the same way. The selection belongs to the project, so it is shared by all views. In a floor plan a left drag pans instead of orbiting. The shell only draws the tabs the app lists.
- **Sheets** (`document::Sheet`): A3 landscape, numbered `A01`, `A02`, ...; each opens in its own tab. The bar above the paper places any floor plan in the middle at 1:100. A placed plan is drawn as the line segments where its cut height meets the shown layers (`drawing::cut_at`, computed once per plan and cached in the app). A drag moves a placed view; it stays on the paper.
- **Dimensions** (`document::Dimension`): the **Dimension** tool of the sheet bar; two clicks on a placed plan, snapping to the ends of its cut lines within 8 px, add a dimension to the plan in model coordinates, so it shows wherever the plan is placed and moves with it. Drawn 6 mm off the measured points with extension lines, slashes and the length in millimetres. Escape drops a first point, then switches the tool off.
- **The views window** (desktop): the shell runs as an iced daemon that opens its main window itself and, while a floor plan or sheet is open, a second window with a tab per plan, section and sheet. A view there has its own camera and section box (`App::with_plan` swaps them in for its pointer), so the 3D view and the plan are on screen together and share the selection both ways. The renderer keeps render targets per viewport size (up to three), so the two windows never reallocate them. The browser keeps the tabs in its one window.
- **Editing on the sheet**: a click picks a placed view or a dimension. A picked view gets its scale (1:20 to 1:1000), a floor plan its cut height (cut again; its tab's section box follows), a handle on each side of its frame to crop it (`PlanView::crop`, in drawing coordinates; the drawing stays where it is, its tab's box follows), and Take off sheet; Delete removes what is picked. The paper is A4 to A0, landscape.
- **Lines below the cut**: the outlines and creases (over 20°) of the triangles between a plan's view depth and its cut, seen from above, thin and grey under the cut (`drawing::plan_at`; edges matched by their ends rounded to 0.1 mm). About 5 ms for a 46k-triangle model.
- **Sections and elevations** (`document::SectionView`): a line in plan looking to its left, cut along it, seen to a depth beyond it; drawing coordinates are u along the line and v up. **Make four elevations** puts one 2 m outside the model on every side; the sheet's **Section** tool draws one with two clicks on a placed floor plan (named Section A, B, ..), and every placed plan shows its line dash-dot with letter and arrows. Their drawing (`drawing::section_at`) keeps of the edges beyond the line only what no nearer face covers, through a 2048-pixel depth image of the faces; about 60 ms for an elevation of a 46k-triangle model. They open like plans (level, in their box) and take dimensions and crops.
- **IFCX** (`export::ifcx`, Save as IFC5/IFCX): the model as IfcX nodes (project, building, the levels as storeys, one node per element with class, name, properties, colour and mesh, placed under its storey), and the document's plans, sections and sheets as `o3v::` nodes, which the IFCX reader keeps and the document takes over when the file is opened again (a plan finds its level by name). A 3.6 MB IFC saves as IFCX in 0.18 s and opens again in 0.03 s.

## Calls made by the builder

- A view on a sheet is a line drawing of the cut, not a picture of the 3D view: the renderer holds one set of uniforms, so it draws one view at a time, and a line drawing is what a sheet needs and what dimensions snap to.
- In the browser, floor plans and view tabs share one viewport whose camera and section box are swapped per tab; on the desktop the views window draws them in a second viewport.
- Hidden lines are removed for sections and elevations, not yet for the lines below a floor plan's cut.
- IFC stays the default of Save as; IFCX is the second choice until the project owner makes it the default.
- Default cut 1.2 m, view depth 0.3 m below the level, scale 1:100, A3 landscape, dimension text in whole millimetres, horizontal.
- The cut is computed from the layers shown when the plan is first placed; hiding a layer later does not change it yet.

## Not yet

3D views on sheets, drawing properties (filters per category, hatching of cut faces, line weights), section depth and view range typed in, georeferencing in the IFCX file, and IFCX as the default save format.
