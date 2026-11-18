# Every file format's reader in its own repository

**Status:** rule of 2026-11-18, with the inventory of that day (main at `bed42e6`). The moves are
made one format at a time; the table says how far each is.

## Intent

Open BIM Suite opens files through 44 readers. The knowledge of a format (how its container is
laid out, what its records mean, how its drawings, tags and schedules are put together) is not
the app's business. The owner wants it kept apart, cleanly:

- **one format, one library, one repository**, named after the software that writes the format:
  `<software>-dll` (`revit-dll`, `rhino-dll`, `tekla-dll`);
- **the Revit reader's repository is the model** for how such a repository is built;
- **the app keeps an adapter** per format and nothing else.

Over time format knowledge has leaked into the app, from the Revit reader most of all and from
other formats too. This note says what belongs where, what there is today, and in what order it
moves out.

## The rule

1. A reader lives in its own repository `<software>-dll` of the OpenAEC Foundation. For a format
   that no one program owns, the repository is named after the format (`ply-dll`).
2. The repository is a Cargo workspace laid out as `revit-dll` is, with the format's short name
   as the prefix of its crates:

   | Crate | What it holds |
   |---|---|
   | `<x>-core` | The container: the file's parts, streams, tables and compression. |
   | `<x>-read` | The readers: what the file's records mean, read into the neutral model. |
   | `<x>-model` | The neutral model the reader returns: plain types that know nothing of the app. |
   | `<x>-export` | Writers from the neutral model (meshes, IFC, DXF), where the format has them. |
   | `<x>-ffi` | The C interface, built as `<x>.dll`, for programs not written in Rust. |
   | `<x>-cli` | A command line tool to look into a file. |

   A small format may leave out the crates it has nothing for; it does not merge them.
3. The reader depends on nothing of the app. It returns its own plain types: meshes with their
   names, colours and properties, and where the format has them views, sheets, drawings, texts,
   dimensions, tags, schedules, levels and grids.
4. The app holds one adapter per format in `src/loaders/`: it reads the file's bytes, calls the
   reader, and copies the reader's types into the app's model (`Layer`, views, sheets, drafts).
   An adapter names no record, field or class of the format, does no decoding and makes no
   decisions about the format. `src/loaders/skp.rs` is the example: 26 lines.
5. A fix or an addition to how a format is read is made in the reader's repository, released
   there, and reaches the app by moving the pinned revision in `Cargo.toml`. It is never made in
   the adapter.
6. A reader that others have published as a crate (glTF, OBJ, STL, LAS, E57, Shapefile, USD,
   STEP, IFC) is already apart; its adapter follows rule 4.

## What there is today

Lines of Rust in `src/loaders/` per format, tests included.

### Formats of one program

| Format | In the app | Its repository | Laid out as the model | To do |
|---|---:|---|---|---|
| Revit (`.rvt`, `.rfa`) | 6,346 in 7 files | `revit-dll` | yes | Move what the app knows of Revit's drawings, tags, schedules, filters, phases, fonts, images and browser into the reader; the adapter only maps. |
| Archicad (`.pln`, `.pla`) | 5,995 | none | | New repository; the whole reader moves. |
| Allplan (`.ndw`) | 4,006 | none | | New repository; the whole reader moves. |
| Coordination model (`.smc`) | 2,674 in 3 files | none | | New repository; the whole reader moves. |
| MicroStation (`.dgn`) | 2,644 | `microstation-dll` | no `dgn-core` | The repository's reader is older than the app's: bring it level first, then replace the app's reader (pull request #85 did this on 5 November and no longer merges). |
| Rhino (`.3dm`) | 1,600 | `rhino-dll` | yes | The adapter still decides and converts too much: move that into the reader. |
| FreeCAD (`.FCStd`) | 1,088 in 2 files | none | | New repository. |
| FBX (`.fbx`) | 949 | none | | New repository. |
| Blender (`.blend`) | 749 | none | | New repository. |
| Analysis text format (`.e2k`) | 645 | none | | New repository. |
| Tekla Structures (`.db1`) | 520 | `tekla-dll` | yes | Slim the adapter. |
| Vectorworks (`.vwx`) | 493 | `vectorworks-dll` | yes | Slim the adapter. |
| Vertex BD and BTL (`.vxz`, `.vxm`, `.btl`) | 339 | `vertex.dll` | one crate, loaded at run time | Lay the repository out as the model and name it `vertex-dll`. |
| Project packages (`.tbp`) | 322 | none | | New repository. |
| 3DS (`.3ds`) | 239 | none | | New repository. |
| SketchUp (`.skp`) | 26 | `sketchup-dll` | one crate | Lay the repository out as the model. The adapter is as it should be. |

Together this is some 30,000 lines of format knowledge in the app.

### Formats that no one program owns

| Format | In the app | Reader | To do |
|---|---:|---|---|
| VRML and X3D (`.wrl`, `.x3d`) | 1,024 | in the app | New repository. |
| PLY (`.ply`) | 299 | `ply-dll` (one crate) | Slim the adapter; lay the repository out as the model. |
| PCD, PTX, XYZ, ASC grid, OFF, AMF, 3MF, COLLADA, LandXML, CityGML, CityJSON, GeoJSON | 145 to 570 each | in the app | Each a small reader: a repository of its own when it is next worked on. |
| IFC, IFCX, STEP, glTF, OBJ, STL, USD, LAS and LAZ, E57, Shapefile, DWG and DXF | 125 to 936 each | published crates and `ifcx-rs` | Adapters; check each against rule 4 when it is next worked on. |
| URDF (`.urdf`) | 112 | `crates/urdf-read`, on the published `urdf-rs` (Apache-2.0): it places the links by their joints, meshes the solids and finds the mesh files | A crate in the suite for now, by the owner's choice: it uses nothing of the app, so it moves one to one into a repository of its own (`urdf-dll`) later. The adapter reads the mesh files with the app's own readers. |

## Moved so far

- 2026-11-18, Revit: which elements are joined with which (`rvt_read::joins`; 48 lines and one
  test left the app).
- 2026-11-18, Revit: the images of a family and the image a parameter names (`rvt_read::images`),
  and whether a wall or floor carries load (`rvt_read::structure`; 152 lines and two tests left
  the app, which keeps how it adds `LoadBearing` to its own properties).
- 2026-11-18, Revit: the cut patterns of the materials (`rvt_read::materials`) and the phasing
  of the project and its views (`rvt_read::phases`; 205 lines and three tests left the app). The
  reader returns plain types of its own here, and the app copies them into its `Pattern`, `Grid`
  and `Phasing`.
- 2026-11-18, Revit: how the browser orders the views (`rvt_export::browser`) and the typefaces
  of the texts (`rvt_read::fonts`; 429 lines and three tests left the app, which copies the
  reader's `Organization` and `FontFace` into its `ViewOrder` and `Face`).

## Order

1. **Revit**, in slices: its repository is the model and its leak is the largest. Each slice is
   a pull request in `revit-dll` and one here that moves the pinned revision and deletes the
   moved code.
2. **Archicad** and **Allplan**: whole readers that only need a neutral model to return.
3. **MicroStation**: bring `microstation-dll` level with the app's reader, then switch.
4. **Rhino**, **Tekla Structures**, **Vectorworks**: slim the adapters.
5. The coordination model, FreeCAD, FBX, Blender and the smaller ones.
6. `sketchup-dll`, `ply-dll` and `vertex.dll`: lay them out as the model.

A new private repository needs a deploy key and a secret before the builds on GitHub can fetch
it; the owner sets those.

## While the moves are under way

- No new format knowledge goes into `src/loaders/`. A change to how a format is read goes to its
  repository, or waits until it has one.
- A file that is being moved is not changed here in the same days: say in the pull request which
  files a slice takes.
- The readers' tests and sample files move with them; what stays here tests the adapter with a
  small file.
- The readers of MagicaVoxel, URDF and OpenDRIVE (2026-11-18) are crates in this repository's
  `crates/` for now, by the owner's choice, not repositories of their own. Each uses nothing of
  the app and has the name it will keep (`vox-read`, `urdf-read`, `xodr-read`), so it moves one
  to one into its own `-dll` repository later.
