# Views, sheets and dimensions

**Status:** built in small steps on 2026-11-07, each shown to the project owner in the running app; follows [levels](2026-11-07-levels-design.md).

## Intent

After levels, the project owner wants to make drawings in the viewer: views of the model (floor plans per level first), sheets with those views placed on them, views moved over a sheet, and dimensions. Within one project there are several views, each in its own tab; selecting in one view selects in all of them. Everything lives in the IfcX project document, which will be saved as `.ifcx` later.

## What exists

- **Floor plans** (`document::PlanView`): made per level in the explorer's **Views** tab, named after the level (`<level> (2)` for a second one). A plan looks from 0.3 m under its level up to a cut 1.2 m above it. A plan goes with its level.
- **View tabs** (`app::DocTab`): the pinned 3D view, then the open floor plans, schedules, Revit sheets and document sheets. Each floor plan tab keeps its own camera and section box (`ViewSlot`), restored when its tab shows again; the 3D view keeps its own the same way. The selection belongs to the project, so it is shared by all views. In a floor plan a left drag pans instead of orbiting. The shell only draws the tabs the app lists.
- **Sheets** (`document::Sheet`): A3 landscape, numbered `A01`, `A02`, ...; each opens in its own tab. The bar above the paper places any floor plan in the middle at 1:100. A placed plan is drawn as the line segments where its cut height meets the shown layers (`drawing::cut_at`, computed once per plan and cached in the app). A drag moves a placed view; it stays on the paper.
- **Dimensions** (`document::Dimension`): the **Dimension** tool of the sheet bar; two clicks on a placed plan, snapping to the ends of its cut lines within 8 px, add a dimension to the plan in model coordinates, so it shows wherever the plan is placed and moves with it. Drawn 6 mm off the measured points with extension lines, slashes and the length in millimetres. Escape drops a first point, then switches the tool off.

## Calls made by the builder

- A view on a sheet is a line drawing of the cut, not a picture of the 3D view: the renderer holds one set of uniforms, so it draws one view at a time, and a line drawing is what a sheet needs and what dimensions snap to.
- Floor plans and view tabs share one viewport whose camera and section box are swapped per tab, instead of a second GPU viewport.
- Default cut 1.2 m, view depth 0.3 m below the level, scale 1:100, A3 landscape, dimension text in whole millimetres, horizontal.
- The cut is computed from the layers shown when the plan is first placed; hiding a layer later does not change it yet.

## Not yet

Lines below the cut (projection), changing the cut, scale or paper, removing a view from a sheet or a dimension in the UI, other view kinds (sections, elevations, 3D views on sheets), cropping views, and saving the document as `.ifcx`.
