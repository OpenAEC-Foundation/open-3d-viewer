# The IFC tab

**Status:** built on 2026-11-09 in small steps, each shown in the running app; follows [views, sheets and dimensions](2026-11-07-views-sheets-design.md).

## Intent

IfcX (IFC5) is the viewer's own file. The project owner wants to see how such a file is built up: its full text, its nodes and how they hang together, the entries as a table, and the relations between the elements.

## What exists

- **The tab** (`app::ifc`, `DocTab::Ifc`): **IFC structure** in the ribbon (group IFC) opens it. Like a schedule, it shows under the 3D view, so a selection is seen in both at once. Opening it again reads the file again, so it follows the model and its document. The IFCX it shows is the file the model was opened from (desktop, one model), else what **Save as** writes for the model and its document (`export::ifcx`).
- **Taking the file apart** (`ifcx_structure`):
  - The text as lines, formatted from the text itself, so members keep the file's order. An object opens up with a member per line; a list holding no object (a mesh's points) stays on one line, shown up to 1,500 characters with its length after it. Each line knows the `data` entry it belongs to.
  - The entries: path, children, inherits, attributes (a short form of each value), class and name, and the line each starts on.
  - The nodes: the entries of one path put together, the later winning and `null` taking a child, inherit or attribute out.
  - The roots: paths that no node holds or inherits.
  - The tree from the roots along children and inherits. A row is keyed by the names from its root, so the same node can hang in two places. A node that holds itself is not opened again.
  - The links, named after the IFC relation they stand for:
    - a spatial node holding a spatial node is `IfcRelAggregates`;
    - a spatial node holding an element is `IfcRelContainedInSpatialStructure`;
    - an element holding an opening is `IfcRelVoidsElement`, and an opening holding an element is `IfcRelFillsElement`;
    - an element holding an element is `IfcRelAggregates`;
    - a child without a class is a part (its geometry);
    - an inherit of a classified node is `IfcRelDefinesByType`, and of a material `IfcRelAssociatesMaterial`.
- **Four views**: Text, Nodes, Table and Relations. Relations has a filter per relation with its count. Only the rows in sight are drawn. A 4 MB IFCX of 15,000 lines and 842 nodes opens in 0.12 s.
- **Selection both ways**:
  - A node picked in any view selects its elements in the model: it and what it holds along children (a storey: all its elements). A node without elements (a body) selects the element it is part of.
  - An element selected in the model picks its node, opens the tree down to where the node first hangs, and scrolls the shown view to it when it is out of sight.
  - Elements are matched to nodes by the `Id` the IFCX reader keeps (the node's path), or for written files by the path the writer gives each element (`export::ifcx::element_path`).

## Calls made by the builder

- A document tab under the 3D view, not a tab of the right pane: the text and the table need the width, and the 3D view stays in sight for the selection.
- The text is formatted from the file's own text, not from a parsed value: the JSON reader sorts the members of an object, and the file's own order is what the tab is for. The IFCX written by Save as has its members sorted because the writer builds it from a sorted value; its text shows them that way.
- One IFC relation name per link, worked out from the classes at both ends. IfcX holds no relation objects; children and inherits carry them.

## Not yet

Searching the text or the tree, editing the file in the tab, the files an IFCX imports shown with it, and binary IFCX.
