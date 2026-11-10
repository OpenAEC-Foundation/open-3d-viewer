# Libraries and systems for the modeller

**Status:** design, 2026-11-09. The first parts are being built: materials, profiles and layered
build-ups. The building-system generator and the kernel and Model tab integration wait for the
project owner (see [Questions for the project owner](#questions-for-the-project-owner)).

## Intent

Open 3D Viewer Studio is growing into a BIM modeller ([tile modeller](2026-11-09-tile-modeller-design.md)).
The project owner wants the ideas of Open BIM Studio in it. Open BIM Studio is written in DynLex,
so its code cannot be reused; its data and its rules can. Two of its parts are asked for:

- **its systems**: the layered wall, floor, roof and ceiling build-ups, and the building systems
  (curtain walls, frames, timber frame, masonry, cladding, hollow-core floors, suspended ceilings);
- **all kinds of libraries**: materials, profiles and the types made of them.

A modeller without libraries places grey boxes of a typed thickness. With libraries, a wall is a
cavity wall of brick, cavity, PIR insulation and sand-lime brick, 360 mm. Its plan is hatched per
layer, its quantities are per material, and its IFC says what it is.

## What Open BIM Studio has

Read from its source (`materials/`, `elements/`, `pattern/`, `structural/`, `bim/`, `ui/editor/`)
and its domain notes (`docs/domain/building-system-generator.md`, `capability-inventory.md`,
`localisation/nl/building-systems/`).

| Part | In Open BIM Studio | Status there |
|---|---|---|
| Library file | An IfcX file holding only types: planar types with layers, opening types, linear types with profiles, drawing types, with their settings. A project file holds its library the same way, as its types. Importing a library adds every type the project lacks, matched by name and role. | built |
| Planar types | `{name, layers: [{material, thickness}], role}`. Role is wall, floor, roof or ceiling, and so the IFC class: `IfcWall`, `IfcSlab` FLOOR or ROOF, `IfcCovering` CEILING. Layers run from the exterior inwards (wall) or from the top down (floor, roof, ceiling). | built |
| Type names | Named by rules, never by hand: the layers' materials from the exterior in, then the total thickness in mm ("Metselwerk-spouw-isolatie PIR-kalkzandsteen 360"). Linear types: material and size ("Beton 300x300", "Beton rond 300"), steel by designation ("HEA 200", "L 80x80x8", "CHS 114.3x5"). Equal names within a role get " (2)", " (3)". | built |
| Starter types | Walls: two cavity walls, a timber-frame outer wall, a metal-stud partition, single-material walls (concrete, sand-lime, brick, timber, steel, insulation). Floors: concrete 150 to 300, hollow-core 200 to 320, timber 200. Roofs: flat concrete with PIR, pitched timber. Ceilings: gypsum. Linear: columns, beams, a mullion, a skirting, a pipe. | built |
| Materials | A name per layer. Recognised: brick, sand-lime, concrete, insulation, PIR insulation, timber, steel, gypsum, air cavity, generic, and more by colour. Each has a 3D colour and a hatch per drawing standard: NEN 47 in colour, DIN 1356, ISO 128 / BS 1192 and ANSI in black on white. IfcX: one node per material (`bsi::ifc::material` with a code, a colour), reached through `inherits`. | built |
| Profiles | The parametric profiles of IFC (rectangle, circle, I, L, U, T, C, Z, rectangular and circular hollow, ellipse, trapezium, arbitrary). The European steel table: IPE, IPN, HEA, HEB, HEM, UPN, UPE, equal and unequal angles, T, SHS, RHS, CHS with h, b, tw, tf, r (EN 10365, EN 10056-1, EN 10055, EN 10210). Beams take every profile; H and hollow sections are columns too. | built |
| Building system types | 43 recipes: a pattern (grid or running bond), bays and rows, joints (width × depth) of a frame material, panels of a planar type, and extra layers at offsets with their own grid and panels. Curtain walls, window frames, timber frame, masonry (waal, dik, stack bond, sand-lime elements), zinc, timber and panel cladding, hollow-core floors 150 to 400, a 600 × 600 ceiling. A system shows its type, and "(custom)" once its own settings differ. | built |
| Building system generator | One recursive grammar for walls, ceilings, roofs, floors, frames and finishes: domain, layer stack with a coverage per layer, pattern graph, assembly tree, connections, fabrication and cutting, overrides with stable semantic identity. An authoring catalogue of 183 concept presets in 43 families, with no performance claims. | designed, not built |
| Other catalogues | Reinforcement steel (B500A/B/C and historical grades, identification only), fasteners, door types. | built |

## Goals

1. A headless library crate, `crates/bim-library`, usable by the kernel, the app, the MCP server
   and other OpenAEC tools. It holds the data and its IFCX form.
2. Materials, profiles and layered build-ups taken over from Open BIM Studio, with its rules:
   names by rule, matching by name and role, a library file of types only.
3. A starter library with Open BIM Studio's starter types, so a new model has walls, floors,
   roofs, ceilings, columns and beams to choose from.
4. Later, after agreement with the kernel's owner: layered planar elements in `bim-core` (one solid
   per layer, each with its material), catalogue profiles on linear elements, and a type chooser
   in the Model tab.
5. Later, after brainstorming with the project owner: the building systems.

## Non-goals (for now)

- Performance values (thermal, fire, acoustic, structural). Open BIM Studio deliberately leaves
  them unknown unless sourced, and so does this crate. No density or λ is invented.
- Manufacturer product packs. Brand-specific window layouts of Open BIM Studio are left out.
- The cutting and stock optimisation of the generator design.
- Door and window types, drawing types, reinforcement and fasteners. They are libraries too, and
  follow once the modeller has openings, drawings by type and reinforcement.

## Anti-goals

- A library that silently changes placed elements. Importing adds what is missing; it never
  replaces a type the project has.
- Invented technical values presented as facts.
- Names of external software or brands in the data.
- A second geometry engine. The crate holds data; geometry stays in `bim-core`. Profile outlines
  come from `bim-structural`, which already turns IFC parametric profiles into outlines with
  their fillets and computes section properties.

## Design

### The crate

`crates/bim-library`, headless, a workspace member. Dependencies: `glam`, `serde`, `serde_json`
and `bim-structural` (profile outlines and section properties). It does not depend on `bim-core`,
so the kernel can depend on it.

| Module | Holds |
|---|---|
| `material` | `Material { key, names (nl, en, de, fr), hatch: HatchKind, colour }`; `HatchKind` (brick, sand-lime, concrete, timber, steel, insulation, gypsum, air, generic); `HatchPattern` per `DrawingStandard` (NEN 47, DIN 1356, ISO 128, ANSI), taken over from Open BIM Studio |
| `buildup` | `Role` (wall, floor, roof, ceiling) with its IFC classes; `Layer { material, thickness }`; `BuildUp { id, role, layers }` with its total thickness, the faces of each layer and its rule name in a language |
| `profile` | `ProfileShape` (the IFC parametric profiles, in metres), the steel table, `LinearType { id, role, material, profile }` with its rule name; outline and properties through `bim-structural` |
| `library` | `Library { materials, buildups, linear_types }`: lookup by key and id, names made unique per role, `merge` (adds what the project lacks, by role and layers), `Library::starter()` |
| `ifcx` | The IFCX form: material nodes and type nodes, a library file of types only, and reading them back from any IFCX file |
| `Lang` | Nl, En, De, Fr, for names. The crate does not depend on the app's `Language`; the app maps one onto the other. |

Units are metres inside the crate, as in the kernel. Open BIM Studio's tables in millimetres are
converted once when the table is read.

### Materials

A material has a stable English key ("concrete", "sand-lime", "insulation-pir"). The key is what
layers, files and other tools refer to; names are for people, in four languages. The hatch kind
says how a cut through it is drawn; the pattern for a kind comes from the drawing standard, so the
drawing settings per country can choose the standard.

The starter materials are Open BIM Studio's: concrete, hollow-core concrete, brick, sand-lime,
insulation, PIR insulation, mineral wool, timber, timber cladding, timber frame with insulation,
OSB, plywood, gypsum board, metal studs with insulation, steel, aluminium, zinc, glass, air
cavity, mortar, glue, grout, screed, mineral fibre, HPL, PVC, roofing, and generic. Their 3D colours
are Open BIM Studio's; where it had none (screed), a neutral colour is chosen.

### Layered build-ups

A build-up is a planar type: a role and its layers.

- Layers run from the exterior face inwards for a wall, and from the top face down for a floor,
  roof or ceiling. `faces()` gives each layer's two faces, measured from that face.
- A build-up is checked: at least one layer, every thickness finite and above 0.1 mm, every
  material known to the library.
- Its name follows the rule. The material names in the chosen language are joined by hyphens, a
  material repeated next to itself counted once, the first letter a capital, then the total
  thickness in millimetres: whole, or with one decimal (a decimal comma in German and French).
  Within a role, equal names get " (2)",
  " (3)" in the order of the library.
- Its `id` is a string, stable and never shown. Elements will refer to their type by id. The
  starter types have readable ids ("wall.cavity-pir-360") that stay the same between versions, so
  the same starter type has the same IFCX path in every project. Types a user makes get a UUID as
  id, so two projects never put different types on one path.
- Importing matches by role and layers: the same materials in the same order, each as thick
  within 0.01 mm. Open BIM Studio matches by name and role, but a name says less than the layers
  (two layers of brick are named as one, and two cavity walls of 360 mm can differ inside), and a
  " (2)" depends on the order in each library.

The starter build-ups are Open BIM Studio's starter planar types (see the table above), in their
layer order and thicknesses.

### Linear types and profiles

A linear type is a role (column, beam, member, mullion, skirting, pipe, pile), a material and a
profile. The profile is a `ProfileShape`: rectangle, circle, I, U, L, T, rectangular hollow,
circular hollow; its outline (with root fillets) and its area and second moments come from
`bim_structural::section`.

The steel table is Open BIM Studio's, with two corrections made from the standards it cites:

- IPN and UPN have tapered flanges (14 % for IPN, 8 % for UPN up to 300 and 5 % above), where Open
  BIM Studio drew them parallel.
- Hot-finished hollow sections have an inner corner radius of 1.0 t besides the outer 1.5 t.

Angles get a toe radius of half the root radius. Every row is checked by `bim_structural`'s shape
check, and the areas of a few sections are tested against their published values.

Steel profiles are named by designation ("IPE 200", "L 100x50x8", "SHS 100x100x5", "CHS 114.3x5");
other linear types by rule: material, then width × depth, or "rond" and the diameter.

The steel table is a catalogue, not 323 starter types: a project takes a type from it when it is
used (`LinearType::steel`), as a column or a beam. The starter library has Open BIM Studio's ten
starter linear types: concrete, steel and timber columns and beams, an aluminium mullion, a timber
skirting and a pipe. Linear types are imported by role, material, profile and designation.

### The IFCX form

As in Open BIM Studio, a library is an IFCX file holding only types, and a project file holds its
library as its types.

- A material is a node with `bsi::ifc::material` (`code` = its key, `uri` = `urn:openaec:material:<key>`),
  `bsi::ifc::presentation::diffuseColor`, `bsi::ifc::prop::Name` (its English name) and
  `openaec::material` (its names, hatch kind and colour). The app's IFCX reader already shows
  `bsi::ifc::material` as the Material property and follows `inherits` to material nodes.
- A build-up is a node with `bsi::ifc::class` (`IfcWallType`, `IfcSlabType`, `IfcCoveringType`),
  `bsi::ifc::prop::PredefinedType` (STANDARD, FLOOR, ROOF, CEILING), `bsi::ifc::prop::Name` (its
  English rule name) and `openaec::buildup` (`id`, `role`, `layers` of material key and thickness).
- A linear type is a node with `bsi::ifc::class` (`IfcColumnType`, `IfcBeamType`, `IfcMemberType`,
  …), `bsi::ifc::prop::Name` and `openaec::lineartype` (id, role, material key, profile).
- Paths are UUIDs (version 8) derived from the key or id, so a library written twice has the same
  paths.
- Reading takes these nodes from any IFCX file and ignores the rest, so a project file read as a
  library gives its types. Numbers are written so that a type read back equals the one written.

### How it fits the kernel and the Model tab

The kernel (`crates/bim-core`) and the Model tab belong to the tile-modeller work. Its owner
agreed to points 2 and 3 below as a small kernel change, built on its stack, with the IfcX round
trip kept, the material per part next to the class, and the thickness of a layered element read
only. What was built:

- `PlanarDef.build_up: Option<BuildUpDef>`: the type's id and its layers (`LayerDef`: material key,
  thickness, `cavity`), copied from the library, so an element is whole without the library and
  its IFCX definition keeps them. `with_build_up` sets the thickness to the layers'.
- `planar_parts` gives one solid per layer that is not a cavity, each with its material; openings
  go through every layer. A wall's exterior is on its right. `Part.material` carries the material
  key. A build-up of cavities only is refused (`Refusal::NoLayers`).
- `ProfileDef::Catalogue { designation, shape }`, its outline from the library's shape within the
  kernel's chord tolerance; `ProfileDef::steel("HEA 200")`.
- In the app, a layered element shows each layer in its material's colour, its Type and Layers as
  properties, and its thickness read only. The Library tab's **Assign to selection** gives the
  selected walls (or slabs, for a floor, roof or ceiling type) the picked build-up. The Wall and
  Slab tools still place elements without a type.

The original proposal:

1. `bim-core` depends on `bim-library`. Its `Model` holds the project's `Library`.
2. `PlanarDef` gets an optional type id. With a type, the thickness is the type's, and
   `planar_solid` gives one part per layer with its material; an air layer gives no solid. A wall's
   exterior is on its right, walking from start to end, as in Open BIM Studio.
3. `LinearDef`'s `ProfileDef` gets a catalogue variant (designation and shape), or `LinearDef` an
   optional linear type id, so a beam keeps "HEA 200".
4. The Model tab gets a type chooser for Wall, Slab, Roof, Ceiling, Column and Beam, and the
   properties show the type and its layers.
5. Saving an IFCX writes the project's library as its types; each element's body per layer
   inherits its material node.
6. Floor plans and sections hatch each layer by its material.

### The building systems: a built-in system generator

There are two kinds of system in Open BIM Studio, and both are asked for:

- **Layered build-ups** are the planar types above. They are in the first parts.
- **Building systems** generate members and panels on a domain. The project owner asked for a
  built-in system generator that works like a layered curtain wall, with a set of proposed
  systems. The kernel's `SystemDef` is a first, simple one (bays, rows, member size, panel
  thickness). Open BIM Studio's recipes need more of it: a running-bond pattern, panels of a
  material, joints set back from the face, extra layers at offsets, and floors and ceilings lying
  level. Its generator design goes much further (openings, prefab units, cutting, overrides).

**System types** (`bim_library::system`, built). A type is a recipe:
- a group (curtain wall, frame, timber frame, masonry, cladding, floor, ceiling);
- the grid of its face (bay and row, centre to centre; stacked rows or a running bond, every second
  row shifted half a bay);
- the members on the grid's lines (width in the face, depth across it, material);
- the panels in its bays (material, thickness, centred on the face);
- layers across the face, each at an offset (to the exterior positive), with a grid of its own,
  members or none, and panels;
- for a floor or a ceiling, how high above its level it lies.

The 26 starter types are Open BIM Studio's recipes with generic names: three curtain walls, three
frames, seven timber frames from its catalogue (below), three brick bonds, sand-lime
elements, zinc, timber and HPL cladding, five hollow-core floors and a 600×600 ceiling. Its window
layouts (casement, tilt-turn, sliding) need a frame hosted in a wall opening with its leaves, and
come later. A type is named by rule, after its members when it is framed ("Vliesgevel aluminium
50×150 h.o.h. 1500", "Houtskeletbouw 38×140 h.o.h. 600"), after its panels otherwise
("Metselwerk 100 220×62.5 halfsteensverband", "Kanaalplaat 260 1200×6000"). An import matches
types by what they make, as build-ups. In IFCX a type is a node of its group's type class with
`openaec::systemtype`.

**In the kernel** (agreed with the kernel's owner; the first two points are built, the second with the kernel's joins):
- `SystemDef` gets an optional `system_type` (`bim_core::system::SystemTypeDef`), copied from the
  library as a build-up is, so the element is whole without the library; `SystemDef::typed` sets
  the spacing, members and panels to the type's. `system_parts` generates from it
  (`bim_core::system`): the running bond (the rows' lines run through, the joints between the
  units stop against them, so they do not run through), the layers across the face, and the
  material of every member and panel. A system of more than 50 000 parts is refused rather than
  generated. In the app, the Library tab draws a type's face from the kernel's own parts, and
  **Assign to selection** gives the selected systems the picked type; a typed system's member and
  panel sizes are its type's, shown and not typed, its spacing still its own.
- Members stay parts of their system, with one join rule per pair of roles (the mullion runs
  through, the transom stops against it), not a join element per crossing. Each crossing goes
  through the kernel's join geometry (`bim_core::join::cuts` and `finish`), so a joint in a system
  looks and behaves as a join element. A type says how the edge members meet at the corners
  (`Corner::Butt`, the member that runs through reaching the corner, or `Corner::Mitre`); the
  aluminium and PVC frames are mitred.
- A generated joint becomes selectable, keyed by the system and its two grid lines, its kind
  stored as an override on the system. Where a system meets another element, the joint is an
  ordinary join element.
- Lying systems need a domain that lies (an outline and a height), and come after upright ones.

### The first full system: the timber frame

The project owner chose the timber frame (houtskeletbouw) as the first system to work out fully.
It follows the HSB family of Open BIM Studio's building-systems catalogue, which goes beyond its
live recipes (studs at 600 with OSB outside and gypsum board inside):

- **The frame** (built). A type's members can be *framed* (`bim_library::Framing`): the plates
  along the foot and the head run through, the studs stand between them, one at either end and
  one at every bay's centre from the start, and noggins stand between the studs at every row up
  from the foot. The frame lies inside the system's face, square cut. A line on an edge member
  needs no member of its own; a stud or noggin that would overlap an edge member stands against
  it. Mineral wool fills the cells.
- **The layers** (built), from the outside in: a breather membrane, the frame with its insulation,
  plywood, a vapour control layer, a service zone (45×45 battens framed at the studs' centres,
  mineral wool between them) and gypsum board. A layer without members lays whole boards (or
  courses of a roll) from the face's start and foot, the last ones cut, rather than dividing the
  face equally. The boards are two bays wide and as high as the frame's rows, so every seam has
  timber behind it (Open BIM Studio's "supported board edges" rule).
- **The types** (built): studs 38 wide and 140, 184 or 235 deep at 600 or 610 centres, and an
  interior wall 38×89 at 600 with gypsum board on either face, as in the catalogue.
- **Openings** (built): a system gets openings, as a wall has, measured from its start and base.
  A framed face frames each one: a king stud either side from plate to plate, trimmers carrying a
  header (the type's nominal size, 200 high, not a sized lintel), a sill under a window, cripple
  studs over the trimmers and at the regular centres above the header and below the sill; the
  bottom plates stop at a door. The insulation stops at the opening, the boards and membranes are
  cut around it into rectangles (the strips beside it, then the pieces below and above it), and
  the cripples over the trimmers carry the seam on the jamb. Only framed systems take openings; a
  curtain wall or masonry with one is refused until their own openings are worked out. The right
  pane adds a door or a window to a timber frame as to a wall.
- **Prefab units** (built): a framing's longest unit (3300 in the catalogue's exterior walls). A
  longer frame is made of as few units as can be, as equal as can be, each with its own plates and
  end studs. A joint lies on a stud line or, where none is clear, right beside an opening's
  framing, clear of its king studs; no board or membrane crosses one. A frame that cannot be split
  so is refused rather than made with a unit too long. The right pane lists a frame's units.
- **Corners** (built): two timber frames on one base and of one height, an end of each within a
  millimetre of the other's and square to each other, make a corner. The longer one runs through,
  its frame as far as the outer face of the other's frame; the other stops against its inner face.
  Of two as long as each other the one made first runs through. Their layers nest around the
  corner without a gap or an overlap: outside, the layers of the one that runs through run on
  around the corner, each as far out as it lies from its own frame, and the other's stop against
  them; inside, each of its layers runs on to what the other has between its frame and that
  layer's depth, and the other's layers start against the layers they would cross. A board inside
  that ends a member's width or more short of the members it is fixed to gets a member behind its
  edge: a corner stud in the frame that runs through, with an insulated pocket between it and the
  end stud, and a batten more in its service zone. A frame keeps its studs at their centres from
  its own start; the battens and the boards' seams follow the studs, the first board cut to reach
  a stud line; the prefab units are those of the frame as long as it is made, and an opening keeps
  clear of a corner stud. A corner is made only where both frames can be made with it and without
  it, so each stays when the other goes; otherwise, and where three frames end on a point, at
  another angle or with unequal heights, each ends on its line as before (a frame that cannot be
  made alone is refused at a corner too). Frames of two types make a corner
  too (an exterior wall and an interior one). The right pane says at which end a frame makes a
  corner and whether it runs through, and lists its units as the corner leaves them.
- **T junctions** (built): a timber frame that ends on the line of another, between that one's
  ends, on one base, as high as it and square to it, stands against its side. It is the inside of
  a corner twice over, back to back. The frame that stands against the other stops against the
  face of the other's frame, and each of its layers, on either side of it, starts against the
  layers of the other it would cross. The other runs through: its frame is whole, and its layers
  on that side are left out across the wall that stands against it, each running on, either side
  of that wall, to what the wall has between its frame and that layer's depth. A stud more stands
  in the frame that runs through either side of the stretch its boards leave open, behind their
  edges, the pocket between the two filled as any bay (its noggins carry the end stud of the wall
  that stands against it); a service zone that is left out ends with a batten either side, as a
  unit does, and gets a batten more where its board ends short of it. The studs, the battens and
  the boards' seams of the frame that runs through stay where they were. A joint between prefab
  units keeps clear of the junction, and so does an opening. A T junction is made only where both
  frames can be made with it and without it; where two frames end on one point of a third's line
  (a crossing), or the junction leaves no room, each is made as before. A frame can carry several
  T junctions along it, on either side, and corners at its ends. The right pane says at which end
  a frame stands against the side of another, and how many stand against its own.
- **Later**: membrane laps and returns, a layer left out on one system (no interior lining), a
  stock list (stud lengths, boards, rolls), crossings and corners at other angles, layers that
  run on behind a wall that stands against them (an airtight layer), and a choice of which system
  runs through.

## Delivery

Small PRs, each with tests, each visible where it can be:

1. **This note and the crate with materials and layered build-ups**, their IFCX form and the
   starter library (#118).
2. **Profiles and linear types**: the steel table, the starter linear types, outlines and
   properties (#121).
3. **A Library tab in the app** (no kernel change): materials with their colour and hatch, build-ups
   drawn as a section through their layers, profiles drawn as their outline; open and save a
   library file. This is what the dev viewer shows first (#129).
4. **Kernel and Model tab**, once agreed (see above): layered walls and slabs, catalogue profiles,
   the type chooser.
5. **System types** in the library: the generator's recipes, the starter types, their IFCX form.
6. **The system generator in the kernel and the Library tab**: typed systems with running bonds,
   layers and materials, drawn in the tab and assigned to selected systems.
7. **Joints in systems** with `bim_core::join`, then lying systems and frames in openings.
8. **The timber frame worked out** ([above](#the-first-full-system-the-timber-frame)): the frame
   and its layers, then openings, then prefab units, then corners, then T junctions, each its
   own PR.

## Calls made in writing this design

The author made these calls, not the project owner. They are open to correction.

- A new crate, `bim-library`, rather than modules in `bim-core`, so the data can be used without
  the kernel and the kernel's owner keeps `bim-core`.
- `bim-library` depends on `bim-structural` for profile outlines and section properties, rather
  than a second implementation of fillets and section integrals.
- Types are named by rules, as in Open BIM Studio, in four languages.
- Matching on import by role and layers, not by name and role as in Open BIM Studio (see
  [Layered build-ups](#layered-build-ups)).
- Type ids are strings: readable for starter types, UUIDs for types a user makes. They are not
  element ids, so a type id in `PlanarDef` would not share the space of `ElementId(u64)`.
- The library's own IFCX attributes (`openaec::material`, `openaec::buildup`) are not Open BIM
  Studio's (`openbim::walltype` with free-text materials); reading its files is a question below.
- IPN and UPN with tapered flanges, hollow sections with an inner radius, angles with a toe radius.
- Material keys in English; the four names are data of the library.
- Screed gets a neutral warm grey; Open BIM Studio had no colour for it.
- The IFCX attributes for build-ups and linear types are in the `openaec::` namespace, next to
  `openaec::element`, because IFCX has no layer-set schema yet.
- System types are named by rule as well, after their members or their panels, and their
  materials, thicknesses and grids come from Open BIM Studio's recipes. Glass is 24 mm, as there;
  zinc is 0.8 mm, as its recipe's name says.
- A frame (kozijn) type is an IfcWindowType and a timber frame or masonry type an IfcWallType;
  the exchange's owner may choose other classes.
- The timber frames take the catalogue's build-up (plywood inside, a breather membrane outside)
  rather than the live recipes' OSB outside, under the same ids. The catalogue gives the membranes
  no thickness: they are 0.5 mm (breather) and 0.2 mm (vapour control), in rolls 1.5 m wide and
  50 m long, laps left out. Boards are 1200×3000 at 600 centres and 1220×2440 at 610; the service
  battens are 45×45; one bottom and one top plate; the interior wall has mineral wool too. All are
  authoring examples, as the catalogue says, not checked structural or building-physics values.
- An opening's header is 200 high in the exterior frames, a plate laid flat in the interior wall
  and a batten in the service zone. The service zone frames an opening as the frame does, with its
  own battens, so two openings stand at least four battens' widths apart. Boards are cut around
  openings into rectangles, the policy the catalogue calls rectangular opening regions; boards cut
  to an L around a corner come later.
- Corners between timber frames are the author's: the catalogue names corner details but gives no
  rule for them. The longer system runs through (the earlier one of two equally long), a three-stud
  corner with an insulated pocket carries the inner board, and layers butt in the order they are
  met, without laps or corner profiles. Frames meet only when they end on one point (within the
  kernel's join tolerance of a millimetre); which runs through is not yet a choice of the user.
  Every frame looks through the model for the frames at its ends when it is made, as walls do.
- T junctions are the author's too: the frame that stands against another reaches that one's
  frame, so every layer of the other on that side is left out across it, the membranes included
  (no layer runs on behind the wall yet), and the frame that runs through gets two studs more
  with a pocket between them rather than one stud laid flat.

## Questions for the project owner

Asked when the project owner is back; the answers change what gets built.

1. **The first building system.** Which system should work end to end first: a suspended ceiling
   (Open BIM Studio's choice), a timber-frame wall with a window, masonry with its bond, or a
   curtain wall?
2. **Which libraries beyond Open BIM Studio's.** Timber sizes, precast concrete, insulation
   products, door and window types? And should there be manufacturer product packs, with their
   names, or only generic types?
3. **Where libraries live.** In the project file only (Open BIM Studio), or also in a shared company
   library that every new project starts from?
4. **Physical values.** Should materials carry density, λ and other values for quantities and
   energy calculations, and from which source?
5. **Free type names.** Names by rule only, or may a user name a type? Open BIM Studio names by
   rule, but lets a type keep a name of its own as a property.
6. **Files of Open BIM Studio.** Should its library and project files be imported here, and ours
   read there? Its types are `openbim::walltype` with materials as free text ("insulation PIR",
   "steel stud CW75 with insulation"), so that needs a second reader that maps texts to keys, and
   an agreement on one namespace for both tools.
