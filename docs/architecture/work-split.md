# Who works on what in the Modeller

**Status:** agreed with the owner on 2026-11-19. The working rules at the end are a proposal of
the same day.

Four agent accounts work on the Modeller side by side. Each has a focus, so that two accounts
do not change the same code at the same time.

## The split

| Account | Focus | Code | First work |
| --- | --- | --- | --- |
| **Quint** | Oversees the whole, and models buildings: walls, floors, roofs, stairs, rooms, doors and windows | `crates/bim-core`, `src/app/modeller.rs`, `src/app/modeller/` | Issue #216: check sloped roofs in the interface, stairs, rooms (a name, a row in the explorer, IFC export) |
| **Fluit** | Structure: the analysis model, frames, supports, loads and results | `crates/bim-structural`, `src/structural/`, `src/app/structural.rs` | Land PR #287 (FEM text models and SAF) and PR #258 (plates, supports, reactions), then issue #217 (`.rf6` with results) |
| **Octaaf** | Library: types, materials, build-ups, profiles and systems | `crates/bim-library`, `src/app/library.rs` | Land PR #259 (the Library product with its editors), then the library editor of issue #204 |
| **Prestant** | Building services: ducts, pipes, cable trays, their fittings and equipment, and the systems they form | nothing yet; a module of its own beside `src/app/modeller/`, and a crate if the routing needs a kernel of its own | A proposal for the first step: what a run of duct or pipe is in `bim-core` terms, and which types it needs from the library |

The house style editor, the second half of issue #204, waits until the library editor is in use.

## How the four depend on each other

The library supplies the types that the other accounts draw with: wall and floor build-ups for
the building, profiles and materials for the structure, duct and pipe sizes for the services. So:

- A new type, material or profile is asked of Octaaf (an issue, or a small pull request that
  Octaaf reviews), not added to `bim-library` on the side.
- A change to the kernel (`crates/bim-core`) or to `src/app/modeller.rs` goes through Quint, in
  the same way. All four build on these two, so they have one owner.
- Structural elements that are also building elements (columns, beams) are drawn with the
  Modeller's tools, which are Quint's; their analysis model is Fluit's.
- Building services are new: there is no code for them yet. Prestant starts with a proposal, and
  agrees with Quint what it needs of the kernel (openings through walls and floors, levels)
  before building it.

## One rule for all four: the base elements

Everything is built from the base elements of `bim-core`: planar, linear and point elements, and
systems made on them. A wall, a floor, a roof and a ceiling without a system are the same planar
element; they differ in their IFC class, not in their code. A new kind of element is first tried
as a base element with a class and a type from the library (issue #302).

## Issues

An issue names its account at the start of its title (`[Quint]`, `[Fluit]`, `[Octaaf]`,
`[Prestant]`). Issues #215 and #216, from before this split, are Quint's.

## Working rules (proposal)

- Each account works in its own branch and worktree, and lands small pull requests on `main`.
- A draft pull request is opened at the start of the work, so the others see what is under way.
- An issue is claimed before work starts on it, so no two accounts take the same one.
- Work outside one's own focus is reported to the account that owns it, not done in passing.
- Quint reviews pull requests that cross a boundary of this split, and updates this file when
  the split changes.
