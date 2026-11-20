# Who works on what in the Modeller

The integration baseline for 20 November 2026 is main after PRs #352–#355 (commit
`8a42ac6`). Start from the latest main that contains this commit. GIC, Validator,
Modeller and Library are products in the same Rust suite.

## Work for the next session

| Account | Starting point | First deliverable and check |
| --- | --- | --- |
| **Quint** — building modeller and shared kernel | #301, with the plan editor in `src/app/drawn_edit.rs`, transforms in `src/app/transform.rs` and sheets in `src/app/paper.rs` | Draw one room of the reference house using the existing wall, grid and align tools. Place its plan and section on a sheet and reopen the IFCX. Record the first operation that prevents completing the drawing. |
| **Fluit** — structure | #342, followed by #343 and #344. `src/app/modeller/structure.rs` already labels beams and derives pile numbers and a schedule from modelled columns below a level. | Give one pile a persistent identity in IFCX. Check that the same pile keeps its number in plan and schedule after reopening. Reuse the existing pile query while extending the model. |
| **Octaaf** — library | #317 and #308. Connections already have kernel geometry. Materials and their stable keys live in `crates/bim-library`. The local `product-tabs` worktree also contains uncommitted hatch editing work. | Compare that hatch work with main before adding another picker. For the connection catalogue, start with an existing fin plate type and test lookup by stable key through a library save and reload. |
| **Prestant** — building services | #319 depends on the connection model in #316 and the catalogue in #317. Prefab panel and floor manufacturing drawings from #353 already live in `src/element_drawing.rs`. | Specify one duct or pipe junction with its member references, catalogue key and dimensions. Agree the shared fields with Quint and Octaaf before implementing routing. |

## Findings from integration

| Area | Existing behavior | Where the next change belongs |
| --- | --- | --- |
| Element drawings | Fluit's `src/app/element_drawing.rs` creates an editable view and sheet for a selected element. Prestant's `src/app/element_drawings.rs` writes fabrication drawings for framed systems and laying plans for floors, using `src/element_drawing.rs`. | Keep the sheet and batch export commands distinct. Share 2D primitives in `src/drawing2d.rs`. Screen marks, sheets and exports now use its single rectangle clipping implementation. |
| Materials | The starter catalogue uses `clt` for cross-laminated timber. The INB template and Dutch layered build-ups use `timber-clt`. They have different display colours. Library lookup uses the exact key. | Preserve both keys when loading existing projects. Before choosing a preferred key for new types, test an IFCX project containing both keys and a project-specific colour. The two catalogue importers now share the same name-to-ID conversion. |
| Heights and annotation | `document::level_text` formats a level mark in metres with three decimals. Structural tags use `modeller::structure::to_level` for signed millimetres. A level mark has its own flag on a document note. | Keep each unit explicit. For #303 and #304, distinguish a persistent label linked to an element from a tag calculated while drawing the plan. |
| Connections | `bim_core::ConnectionKind` already represents end plates and fin plates. The geometry is in `crates/bim-core/src/join.rs`. | Quint owns member references and placement, Octaaf owns catalogue records, Fluit supplies steel geometry, and Prestant supplies the services case. Exercise the same connection identity across these layers. |

PRs #258, #259, #287 and #353–#355 are already in main. The Library stack through #349
is also included. The remaining CAD import work appears in both #89 and the older integration
PR #351. Reconcile it once when CI can read the private dependency. Treat #85 as a
separate DGN reader migration and retain the suite's product tabs when consulting #79.

## How the four depend on each other

The library supplies the types that the other accounts draw with: wall and floor build-ups for
the building, profiles and materials for the structure, duct and pipe sizes for the services. So:

- A new type, material or profile is asked of Octaaf (an issue, or a small pull request that
  Octaaf reviews), not added to `bim-library` on the side.
- A change to the kernel (`crates/bim-core`) or to `src/app/modeller.rs` goes through Quint, in
  the same way. All four build on these two, so they have one owner.
- Structural elements that are also building elements (columns, beams) are drawn with the
  Modeller's tools, which are Quint's; their analysis model is Fluit's.
- Prestant defines the first services junction with Quint, including level references and the openings it requires through walls and floors.

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
