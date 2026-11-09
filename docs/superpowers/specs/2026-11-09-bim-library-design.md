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

### How it fits the kernel and the Model tab (to agree with the kernel's owner)

The kernel (`crates/bim-core`) and the Model tab belong to the tile-modeller work. The proposal:

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

### The building systems (to brainstorm with the project owner)

There are two kinds of system in Open BIM Studio, and both are asked for:

- **Layered build-ups** are the planar types above. They are in the first parts.
- **Building systems** generate members and panels on a domain. The kernel's `SystemDef` is a
  first, simple one (bays, rows, member size, panel thickness). Open BIM Studio's 43 recipes need
  more of it: a running-bond pattern, panels of a type, joints set back from the face, extra layers
  at offsets, and floors and ceilings lying level. Its generator design goes much further
  (openings, prefab units, cutting, overrides).

What the first vertical slice of the systems should be is a decision for the project owner.
Open BIM Studio's design proposes a suspended ceiling first, then a timber-frame wall with a window.

## Delivery

Small PRs, each with tests, each visible where it can be:

1. **This note and the crate with materials and layered build-ups**, their IFCX form and the
   starter library.
2. **Profiles and linear types**: the steel table, the starter linear types, outlines and
   properties.
3. **A Library tab in the app** (no kernel change): materials with their colour and hatch, build-ups
   drawn as a section through their layers, profiles drawn as their outline; open and save a
   library file. This is what the dev viewer shows first.
4. **Kernel and Model tab**, once agreed (see above): layered walls and slabs, catalogue profiles,
   the type chooser.
5. **Building systems**, after brainstorming.

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
