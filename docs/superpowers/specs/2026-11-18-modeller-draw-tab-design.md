# The Modeller's Draw tab: the first tab of Open 2D Studio, in the Modeller

Date: 2026-11-18. Status: being built in slices; the table below says what is there.

## Why

The owner draws in Open 2D Studio (the Tauri version, 0.35) and wants the Open BIM Modeller to
work the same way in a floor plan, a section and on a sheet: the whole of that program's first
ribbon tab, under another name. An earlier comparison brought over the grid tool with its series,
the wall tool on its two letters, the typed length and the snap marks; the drawing and changing
tools of that first tab were left. This document is the list of them, so that it is not made
again from nothing.

The reference is the TypeScript source of Open 2D Studio (`src/components/layout/Ribbon/Ribbon.tsx`
for the tab, the tools under `src/`) and its building extension; the installed program answers on
a local port (`/eval`), which is how its ribbon was read while it ran.

## Decisions

- **Name.** The tab is **Tekenen / Draw** (`RibbonPage::Draw`), second after Model. It takes the
  place of the Annotate tab, whose three groups it holds: one place for what is drawn on a view.
- **Order.** The groups stand in the order of the reference: Selection, Draw, Annotate, Modify,
  then Edit and Clipboard when they have tools.
- **Only what works.** A tool gets its button when it does its work; no button that does nothing.
- **A shape is one thing.** A view keeps what is drawn on it as lines (`lines`), which is what its
  tab, the sheets, the exports and other readers of the file draw. A rectangle, circle, arc,
  ellipse or polyline is a run of those lines, named in `shapes` (`[kind, first, count]` per
  shape in IFCX). A file without `shapes` reads as before; a reader without them sees the lines.
  A click on a line of a shape picks all of it, and Remove takes all of it.
- **Curves are chords.** A circle and an ellipse are 48 chords, an arc as many as its part of the
  circle. Their true centre and radius are not kept yet; a tool that needs them (a centre snap, an
  offset, an export as a true circle) works them out of the chords or waits for them to be kept.
- **Keys.** The two letters of the reference: LI, PL, RC, CI, AR, EL, TX, DI, MV, CO, RO, MM.

## How the tools draw

| Tool | Clicks |
| --- | --- |
| Line | two points; goes on from its end; Esc lets go, Esc again stops |
| Polyline | a point per click; Enter, the right button or a click on the last point ends it, a click on the first point closes it; Esc keeps what is drawn |
| Rectangle | two opposite corners |
| Circle | centre, then a point on it |
| Arc | start, a point on the arc, end |
| Ellipse | centre, the end of one axis, how far the other reaches |

A click lands on the end of a line drawn on the view when it is near one, else on the end of a
line of the view's cut, else where the pointer is.

## The first tab of the reference, and where the Modeller stands

Done: in the Draw tab and working. Elsewhere: the Modeller has it in another place. Open: to build.

| Group | Tool (keys) | State |
| --- | --- | --- |
| Selection | Select (MD) | done: `model.select`, also S |
| Selection | Pan | elsewhere: the middle button drags the view |
| Selection | Select all, Deselect | open (Esc deselects) |
| Selection | Find and replace (Ctrl+H) | open |
| Draw | Line (LI) | done |
| Draw | Polyline (PL) | done |
| Draw | Rectangle (RC) | done, one shape |
| Draw | Circle (CI) | done, one shape |
| Draw | Arc (AR) | done, through three points |
| Draw | Ellipse (EL) | done |
| Draw | Spline (SP) | open |
| Draw | Filled region | open (a view has fills from opened files; none can be drawn) |
| Draw | Line component (DL), L-shape (LS) | open |
| Draw | Text (TX) | done, in the Annotate group |
| Draw | Image (IM), PDF underlay | open in this tab |
| Annotate | Aligned dimension (DI) | done: Dimension |
| Annotate | Linear, Angular (DA), Radius (DR), Diameter (DD) | open |
| Annotate | Spot coordinate (SC), Leader (LE), Label (LB), Table, Cloud | open |
| Annotate | Measure (ME) | elsewhere: the View tab's Measure group |
| Modify | Move (MV), Copy (CO), Rotate (RO), Mirror (MM) | done for model elements and for the line or shape picked on a floor plan (one at a time; not yet on a section or sheet) |
| Modify | Array, Scale (RE) | open |
| Edit | Offset | done on a floor plan: a distance typed in the right pane (mm), a click on the side; corners meet again; no key yet |
| Edit | Explode | done: the shape picked becomes loose lines |
| Edit | Trim, Extend | done on a floor plan: click the line to work against, then the line to trim (on the part that goes) or extend; straight lines only, no keys yet |
| Edit | Fillet, Chamfer | done on a floor plan: click two lines on the side that stays; radius or distance typed in the right pane (mm), 0 makes a corner; the tool stays on for the next pair |
| Edit | Stretch, Split, Align, Break, Join, Lengthen | open |
| Clipboard | Paste, Cut, Copy | open |
| Clipboard | Delete | done: Delete, and Remove in the right pane for what is picked on a view |
| Group | Create, Explode | open |

## Next, in this order

1. More than one line or shape picked at a time (a box, Shift), and the Modify tools on a
   section and a sheet.
2. The drawing tools take the Modeller's snaps and marks (end, middle, grid, on a line) and the
   typed length; the points being clicked move from the canvas into the app for that.
3. Edit: Join, keys for the Edit tools, and trimming against and of curves.
4. Spline, filled region, the other dimensions, leader and cloud.
5. Clipboard, array and scale.
