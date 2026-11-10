# Three products from one code base

**Status:** proposal of 2026-11-09, built in small steps. The switch is in the app, and each
product builds on its own; per module the owner decides where it belongs (the questions at the end).

## Intent

Open 3D Viewer Studio is turning from a viewer into a modeller. The owner wants one code base
that is three products, each with its own build and its own interface:

- **Viewer**: open any BIM or CAD file, look at it, measure it, cut it, read its properties.
- **Modeller**: make a BIM model from the five base elements of `bim-core` (wall, slab, column,
  beam, curtain wall, object, connection), with levels, views and sheets.
- **BIM Collaborator**: where a team checks models together and agrees on what to change.

Not three repositories, not three apps glued together: one `App`, one window, one project. The
switch at the top right of the window moves between the three; each shows its own ribbon, panels
and start screen over the same open project (models, document, selection, camera).

## The rule

A module is **shared** unless it serves one product only. Shared modules carry no product
feature and stay as they are. A module that serves one product moves behind that product's Cargo
feature when it is worth leaving out of the others (a dependency, size, a licence, a command the
other products should not offer). Until then the product decides only what its interface shows.

## Modules

| Module | Viewer | Modeller | Collaborator | Proposal |
| --- | :-: | :-: | :-: | --- |
| `vendor/openaec-ui`: window, tokens, widgets, icons, languages | ✓ | ✓ | ✓ | shared |
| `src/shell.rs`: title bar, product switch, project tabs, Bestand, settings | ✓ | ✓ | ✓ | shared; the ribbon and panels per product |
| `src/loaders/`, `src/model.rs`: every format to one mesh model | ✓ | ✓ | ✓ | shared |
| `src/viewport/`, `view_cube`, `selection`, `section`, `measure/`, `history` | ✓ | ✓ | ✓ | shared |
| `src/app/layers.rs`, properties, explorer (Model, Levels, Views) | ✓ | ✓ | ✓ | shared |
| `src/document.rs`: levels, plans, sections, sheets, dimensions, texts (IfcX shape) | read | edit | read, annotate | shared document; editing belongs to the Modeller (question 3) |
| `src/app/{views,paper,sheets}.rs`, `sheet_layout`, `drawing`, `drawing2d`, `export/{cad,pdf,svg}` | ✓ | ✓ | – | question 3: drawing production in the Viewer, the Modeller or both |
| `src/export/{ifc,ifcx}.rs`, `ifcx_structure`, `src/app/ifc.rs` (IFC tab) | ✓ | ✓ | ✓ | shared |
| `src/app/schedules.rs`, `grids.rs`, `marks.rs`, `annotations` | ✓ | ✓ | ✓ | shared |
| `crates/bim-structural`, `src/structural/`, `src/app/structural.rs` | show | show, later edit | show | shared |
| `crates/bim-core`, `src/app/modeller.rs`: the kernel and the modelling tools | – | ✓ | – | **Modeller**; first module behind `modeller` |
| validator and BCF crates of the draft validator branch | – | – | ✓ | **Collaborator**, behind `collaborator` |
| `src/explode.rs`: exploded view | ✓ | – | ✓ | shared |
| `src/remote.rs`, `src/mcp.rs`, `open-3d-viewer-mcp` | ✓ | ✓ | ✓ | shared; tools per product later |
| `src/desktop.rs`, `src/web.rs`, `gpu`, `managed`, `bench` | ✓ | ✓ | ✓ | shared platform code |

## Builds

Cargo features say which products a build holds; binaries say which product a build is.

| Build | Command | What it shows |
| --- | --- | --- |
| Studio (the app as released) | `cargo build --release` | all three, with the switch; starts in the product used last |
| Viewer only | `cargo build --release --no-default-features --features viewer` | the Viewer, no switch |
| Modeller | `cargo build --release --bin open-bim-modeller` | the Modeller only |
| BIM Collaborator | `cargo build --release --bin open-bim-collaborator` | the Collaborator only |
| Web | `trunk build` | all three; `?product=modeller` starts in that product |

- Features `viewer`, `modeller` and `collaborator` are all on by default. A product binary needs
  its feature (`required-features`), so a build without it leaves the binary out.
- Cargo unifies features within one invocation, so a slim product build is a build of its own:
  `cargo build --release --no-default-features --features modeller --bin open-bim-modeller`. The
  draft validator branch learned the same: one invocation per product.
- A build without any product feature stops with a compile error that says so.
- Each product has its name (window title, About) and its id in the installer catalogue, so an
  update notice only offers that product. The Studio keeps the viewer's name and id
  (`open-3d-viewer`), so installed copies keep updating. Preferences stay shared: one family, one
  user. Remote control (MCP) goes to the program started last, as with two viewers today.
- CI checks the product binaries and each feature on its own (`cargo check` for wasm32, which
  checks only the app crate again), besides the tests of the default build.
- Releases build and ship the Studio and the MCP server only; packaging the product binaries waits
  for question 4.

## What each product shows

| | Viewer | Modeller | BIM Collaborator |
| --- | --- | --- | --- |
| Ribbon | **Start**: file, display, show, IFC, camera, selection, section, measure, more | **Model**: new, open, save; planar, linear, system, point, connection, modify. **View**: the Viewer's display, camera, section and measure groups | **Collaborate**: file (open, add), issues, check, comments, share. **View** |
| Left panel | Explorer: Model, Levels, Views | Explorer: Model, Levels, Views | Collaboration: the open models, issues, checks |
| Right panel | Properties, Layers | Properties (the element's definition), Layers | Properties, Layers |
| Start screen | Open a model | New model, or open one | Open the project's models |

What is the same in all three: the title bar with the switch, project tabs, document tabs (3D,
plans, sections, sheets, schedules, IFC), the status bar, Bestand and the settings. Switching
keeps the project; it ends a modelling tool that was on.

## The BIM Collaborator: a proposal

Not defined yet. What it would hold, in a proposed order:

1. **Federated models**: the discipline models of a project open together (Add exists), each
   with its own visibility, version and origin.
2. **Model checking against IDS**: load an `.ids`, check the models, see per specification what
   passed, failed or could not be checked; failed elements selected and coloured. The validator
   core of the draft validator branch does this natively.
3. **Issues (BCF)**: an issue on a selection with its viewpoint (camera, section, what is
   hidden), comments, assignee, status, priority, due date; `.bcfzip` in and out; a click goes
   to the viewpoint. A failed check becomes an issue in one step.
4. **Comments and markup**: notes in 3D on an element, and on a sheet.
5. **Shared models**: a project shared with the team (a folder, a BCF server, the OpenAEC
   SuperCloud), issues kept in step.
6. Later: model versions compared, clashes between models.

The first step of the Collaborator is a skeleton: its ribbon with the planned commands shown
but not yet working, and a panel that says what comes.

## Questions for the owner

1. **Collaborator**: are these the right parts, and which comes first? Proposal: IDS checking and
   BCF issues, from the draft validator branch.
2. **Names**: working names are *Open 3D Viewer Studio* (the Studio and the Viewer), *Open BIM
   Modeller* and *Open BIM Collaborator*. Other names?
3. **Drawings**: sheets, dimensions and drawing export: Viewer, Modeller, or both?
4. **Installers**: publish the Modeller and the Collaborator as installers of their own now, or
   only the Studio with its switch until they are further?
5. **The draft validator branch**: move its validator and BCF crates into the Collaborator on
   this structure?

## Calls made in writing this

- One `App` and one window; the product is a property of the window shell (`Studio`), not of the
  project, so every project tab shows in the product chosen.
- The Viewer loses the Model tab; modelling is the Modeller's.
- Product names on the switch are names, the same in every language.
- Code moves behind features module by module, starting with `bim-core` and the modeller; the
  features so far decide which products a build offers, and leave the code where it is.
