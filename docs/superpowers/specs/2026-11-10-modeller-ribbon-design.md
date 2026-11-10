# The Modeller's ribbon: tabs in the order other BIM modellers use

## Question

How should the Modeller's ribbon be laid out so that a user who knows another BIM modeller finds
the commands where they expect them, without giving up the five base elements of `bim-core`?

## What other BIM modellers do

The notes below come from each product's documentation and, where the documentation is thin,
its source code. Each is marked **[D]** (documented) or **[I]** (inferred).

- **Revit.** The Architecture tab holds Build (Wall, Door, Window, Component, Column, Roof,
  Ceiling, Floor, Curtain), then Circulation, Model, Room & Area, Opening, Datum (Level, Grid) and
  Work Plane. Annotate holds Dimension, Detail, Text and Tag; View > Create holds 3D, Section,
  Plan Views, Elevation and Schedules, and Sheet Composition follows; Modify holds Move, Copy,
  Rotate, Mirror, Align, Trim, Split and Delete. [D] Autodesk's 2026 shortcut list gives each
  command's tab and panel:
  <https://damassets.autodesk.net/content/dam/autodesk/www/revit-downloads/2026/rvtkeyboardshortcuts.pdf>.
  The left-to-right panel order is [I].
  - The Type Selector tops the Properties palette while placing [D]:
    <https://help.autodesk.com/cloudhelp/2015/ENU/Revit-GetStarted/files/GUID-A764EA7A-FE26-469B-857C-F3A70812FC34.htm>.
  - The wall's Options Bar holds Level, Height, Location Line, Chain, Offset and Join Status [D]:
    <https://help.autodesk.com/cloudhelp/2018/ENU/Revit-Model/files/GUID-05BAFEAA-5186-484E-80F4-8D900C454748.htm>.
  - A door goes in with a click on a wall, which cuts the opening [D]:
    <https://help.autodesk.com/cloudhelp/2024/ENU/Revit-ArchDesign/files/GUID-7E952007-8A5A-4693-8E0F-A847F72F7617.htm>.
  - A selected element gets drag controls and shape handles [D]:
    <https://help.autodesk.com/cloudhelp/2024/ENU/Revit-GetStarted/files/GUID-EECE4EA1-DDBB-49A4-88DA-733DE0DE0B9E.htm>.
- **Archicad.** The Toolbox's sections are Select, Design, Document and More [D]:
  <https://help.graphisoft.com/AC/18/INT/AC18Help/01_Configuration/01_Configuration-74.htm>.
  Design runs Wall, Curtain Wall, Door, Window, …, Column, Beam, Slab, Stair, Roof, Object [I].
  - The Info Box shows the active tool's settings [D]:
    <https://help.graphisoft.com/AC/26/INT/_AC26_Help/040_ElementsVB/040_ElementsVB-3.htm>.
  - Doors and windows go only in walls [D]:
    <https://help.graphisoft.com/AC/30/INT/Topics/040_ElementsVB/040_ElementsVB-306.html>.
  - Stories, sections, elevations and layouts live in the Navigator [D]:
    <https://help.graphisoft.com/AC/28/INT/_AC28_Help/030_Interaction/030_Interaction-4.htm>.
- **Bonsai.** The toolbar left of the viewport runs Explore, Wall, Slab, Door, Window, Column,
  Beam, …, Annotation. The viewport header carries the type to place and its height or level,
  and, for a selection, its edit tools [D]:
  - <https://docs.bonsaibim.org/studio/reference/toolbar/index.html>
  - <https://raw.githubusercontent.com/IfcOpenShell/IfcOpenShell/v0.8.0/src/bonsai/bonsai/bim/module/model/workspace.py>
- **FreeCAD BIM.** The 3D/BIM toolbar runs Site, Building, Level, Space, then Wall, Curtain Wall,
  Column, Beam, Slab, Door, Window, …, Stairs, Roof. An Annotation toolbar (dimensions, text, grid,
  section plane, sheet), Modify toolbars and a Manage toolbar follow. A window's host is the wall
  face clicked [D]:
  - <https://raw.githubusercontent.com/FreeCAD/FreeCAD/main/src/Mod/BIM/InitGui.py>
  - <https://raw.githubusercontent.com/FreeCAD/FreeCAD/main/src/Mod/BIM/bimcommands/BimWindow.py>
- **Vectorworks Architect.** The Building Shell tool set runs Wall, Door, Window, Column, Slab,
  Stair, …; Dims/Notes holds dimensions, tags and the section line. The Tool bar holds the wall's
  control line and style [D]:
  - <https://app-help.vectorworks.net/2026/eng/assets/docs/Commands_Tools2026.pdf>
  - <https://app-help.vectorworks.net/2026/eng/VW2026_Guide/Walls/Creating_walls.htm>
- **Allplan.** The Architecture role's Elements task runs Components, Opening Elements, Roof,
  Stair, Railing, Views and Sections, then Change, Edit, Measure and Annotations [D]:
  <https://help.allplan.com/Allplan/2026-1/1033/Allplan/92503.htm>.

**What they share** [I]:
- The order: building elements (walls first, then columns, beams, slabs and roofs), then openings
  hosted in walls, then circulation, then annotation (dimension, text, tag), then views and
  sheets; modify comes last or appears with the selection.
- Views, sheets and levels always have a browser of their own.
- The type and the placement options of the tool on are in one strip beside the canvas.

**Where they differ** [I]: where levels and grids sit (Revit's Datum panel, a dialog in Archicad
and Allplan, FreeCAD's first toolbar), and one ribbon per discipline (Revit, Allplan) versus one
tool palette (Archicad, Vectorworks, Bonsai, FreeCAD).

## The layout

The Modeller's ribbon has four tabs after File:

| Tab | Groups |
| --- | --- |
| **Model** (Modelleren) | Planar (Wall, Slab) · Linear (Column, Beam) · System (Curtain wall) · Point (Object) · **Openings (Door, Window)** · Connection (Butt, Mitre, Cut) · Modify (Edit, Delete) |
| **Project** | Levels (Level) · Views (Floor plan; Structural plan, Ceiling plan, Section) · Sheets (Sheet) · Library (Library) |
| **Annotate** (Annoteren) | Dimension (Dimension; Standard dimensions) · Text (Text) |
| **View** (Beeld) | the Viewer's display, camera, section and measure groups, as before |

- **Base elements stay the structure of the Model tab**, in the shared order of what is built.
  The openings follow them, as in every product above.
- **New, Open and Save as leave the Model tab.** They are on the File page and in the title bar,
  as in the other products, where the ribbon holds modelling commands only.
- **Door and Window are tools.** A click on a wall puts the opening there, centred on the point
  clicked. A door stands on the wall's base, a window on its sill. The opening keeps 100 mm of
  wall at the wall's ends. While the cursor is over a wall, the opening shows there.
- **The Project tab gathers what the explorer holds**, as the browsers above do: a new level
  opens its form in the Levels tab, a plan kind lists the levels to make one of in the Views tab,
  and Library opens the Library tab. The explorer opens if it was closed.
- **Annotate works in the floor plan, section or sheet shown**; its commands are greyed out
  elsewhere. Section draws a section on a floor plan, so it is a view: it sits with the views.

## Not in this slice

- **A placement strip.** All of the products above give the tool on a type and its options
  (height, location line, offset) in one strip. The Modeller's tools still place fixed sizes.
  That strip is the next step: the properties pane, while a tool is on, showing the type and the
  values the next element gets.
- **Move, Copy, Rotate and Mirror** for a selection, and a contextual Modify group.
- **Roof, Stair and Railing**, and **Grid** as a datum command.
- **Duplicate controls.** The bar over a floor plan's tab still has its own Dimension, Text and
  Section buttons. The Viewer needs them, since it has no Annotate tab.

## Verdict

Adopt this layout for the Modeller now. Revisit it when the placement strip and the modify
commands come, which add groups but do not move these.
