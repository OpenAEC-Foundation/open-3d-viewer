# Levels: the first piece of edit mode

**Status:** approved by the project owner on 2026-11-07, being built.

## Intent

The native file format of Open 3D Viewer Studio is going to be IfcX (IFC5). Edit mode starts with the building's levels: when a model has levels (Revit, IFC, IFCX, Archicad, Allplan), the app recognises them as IfcX levels and lists them; when it has none, or one is missing, the user adds them. Floor plans per level come next, then cropping views, sheets and placing views on sheets. Levels are the base those steps hang on.

## Goals

- **Recognise** the levels of every format that has them, including IFC (`IfcBuildingStorey`, not read until now) and IFCX (`IfcBuildingStorey` nodes, not read until now).
- **One document in IfcX form.** The app keeps a document beside the model, in the shape of IfcX nodes. Recognised levels and levels the user adds both live there; everything later (floor plans, sheets) is added to the same document.
- **A Levels tab** in the left panel, next to the explorer's current content, that lists the levels from high to low with name and elevation.
- **Add a level**: a name and an elevation in metres; the new level appears in the list in its place by elevation.
- **Remove a level the user added.** Levels read from the model are read-only.
- **Keep added levels when saving.** Until IfcX writing exists, *Save as IFC* writes the added levels as storeys next to the model's own.

## Non-goals (for this piece)

- Floor plans, views, cropping, sheets: the next tabs, each a design of its own.
- Writing `.ifcx` files. The document can already turn its levels into IfcX nodes (tested), but there is no save command for it yet.
- Editing or deleting levels that came from the model, renaming levels, and moving elements between levels.
- Assigning IFC or IFCX elements to their storey (a `level` property per element, which the exploded view and the IFC export would use).

## Anti-goals

- **A second list that drifts.** The Levels tab and the IFC export read the same document; there is no copy kept in step by hand.
- **A broken browser build.** Everything works in the wasm32 build too.
- **Losing what the user made without a reason.** Added levels survive adding a model and re-layering IFCX files; they go only when another model is opened in place of this one (a new project) or the app closes. Saving as IFC keeps them.

## Behaviour

### The left panel

The explorer gets two tabs at the top: **Model** (what the explorer shows today: models, views, schedules, sheets, rooms, layers) and **Levels** (Dutch: *Model* and *Niveaus*, matching the *Niveaus* heading the properties pane already uses). Floor plans and sheets become further tabs later. The tab bar is part of the explorer pane itself, so the stand-alone layout and the OpenAEC shell show the same thing.

### The Levels tab

- A header row: *Levels*, the count, and a **+** button.
- The levels from high to low, each with its name and its elevation (`+3.000 m`, three decimals, as the properties pane shows them). A level the user added carries a small *added* marker and a remove button.
- **+** opens an inline form under the header: a name field (pre-filled with the next free *Level n*, Dutch *Niveau n*) and an elevation field (pre-filled with the highest level plus 3 m, or 0 when there is none), with *Add* and *Cancel*. Enter in either field adds.
- Refusals are shown under the form and nothing is added: an empty name, a name another level already has (case-insensitive), or an elevation that is not a number. A comma is accepted as the decimal separator.
- Without a model the tab says *No model loaded*, like the explorer. A model without levels shows a line saying so and pointing at **+**.

### Where a level comes from

| Format | Source | Elevation |
|---|---|---|
| Revit, Archicad, Allplan | the loaders' levels, as today (`ProjectData::levels`) | as the loader gives it |
| IFC | every `IfcBuildingStorey`, read with the IFC toolkit already in use (`ifc-lite-core`, MPL-2.0) | `Elevation` in metres (unit scale applied), else the Z of the storey's own placement, plus the Z of the building's placement, minus the anchor the loader subtracts: the same frame as the meshes |
| IFCX | every node of class `IfcBuildingStorey` | `bsi::ifc::prop::Elevation` when given, else the Z of the node's composed `usd::xformop` |

The IFC and IFCX loaders put their storeys into `ProjectData::levels` like the others, so the rooms grouping, the exploded view and the IFC export see them too.

## The document

A new module, `src/document.rs`: the project document in IfcX form, headless (no GUI or GPU), so the MCP server and a later `bim-core` crate can take it over.

- `Level { path, name, elevation: f64, added: bool }`. `path` is the IfcX path, a UUID. For a recognised level it is derived from its name, so the same model gives the same paths every time; for an added level from its name and a counter.
- `Document { levels }`, kept low to high. `Document::recognise(&Model)` replaces the recognised levels by the model's and keeps the added ones (an added level whose name the model now has is dropped as a duplicate). `add_level`, `remove_level` (added levels only), `next_level` (the suggested name and elevation).
- `Document::ifcx_nodes()` gives each level as an IfcX node:
  `{"path": <uuid>, "attributes": {"bsi::ifc::class": {"code": "IfcBuildingStorey", "uri": "https://identifier.buildingsmart.org/uri/buildingsmart/ifc/5/class/IfcBuildingStorey"}, "bsi::ifc::prop::Name": <name>, "bsi::ifc::prop::Elevation": <metres>, "usd::xformop": {"transform": <translation to the elevation>}}}`.
  The IFCX loader reads these nodes back into the same levels (tested), which is what "recognised as IfcX levels" means in code.
- The `App` holds one `Document`. Opening a model starts a new one; adding a model, or re-layering IFCX files, calls `recognise` on the combined model.

## Saving

`export::save` and `export::to_bytes` take the document next to the model. The IFC writer takes its storeys from the document's levels (all of them, low to high) instead of `ProjectData::levels`; with no added levels that is the same list as before. The command-line conversion uses the document recognised from the model.

## Tests (written first)

- Document: recognising a model's levels in order with stable paths; adding (sorted in, refusals for empty, duplicate and non-finite); removing only added levels; `recognise` keeping added levels; the suggested next level; IfcX nodes with class, name, elevation and transform.
- IFC loader: storeys with names and elevations in the mesh frame (a storey on a building placed above zero, a wall on that storey whose base matches the level); millimetre files; an IFC exported with levels reads back the same levels.
- IFCX loader: storeys from `Elevation` and from the transform; the document's IfcX nodes read back as the same levels.
- App: the Levels tab lists the levels high to low; **+**, typing and *Add* add a level; a refusal is shown; remove works on added levels only; added levels survive *Add model* and go on *Open*; saving as IFC writes added levels as storeys.

## Calls made in writing this design

These were settled by the builder, not by the project owner, and are open to correction:

- IfcX has no fixed attribute for a storey's elevation yet; the document writes `bsi::ifc::prop::Elevation` and a matching `usd::xformop`, and reads either.
- The tab is called *Niveaus* in Dutch, as the properties pane's heading, and *Levels* in English; the current explorer content becomes the *Model* tab.
- Levels are listed high to low, as the properties pane does.
- No element count per level yet (IFC and IFCX elements do not know their storey yet).
- Added levels are kept in memory and written by *Save as IFC*; opening another model in place of the current one drops them without asking.
- The UUID of a level is derived from its name (and a counter for added ones), not random, so tests and repeated loads give the same paths.

## Follow-ups

- Floor plans: a second tab, a floor plan per level.
- Writing the document as `.ifcx` (and opening it again with its added levels).
- Elements of IFC and IFCX files assigned to their storey (`level` property), for the exploded view, the export and an element count per level.
- Editing levels (rename, change elevation, delete with the model's levels), with undo.
- MCP tools to list and add levels.
