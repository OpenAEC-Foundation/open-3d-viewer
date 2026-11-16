# Modeller: types with layers and materials Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a wall taken over from an opened file is made of what the file's wall was made of: its layers and materials, kept in the project's library, in IFCX and in IFC4.

**Architecture:** the reader of opened files gives each wall its type's layers (`SourceWall::layers`) and the side its exterior is on. Taking a wall over (`src/app/take_over.rs`) asks `src/app/modeller/file_types.rs` for a build-up: the library's materials by name, new ones for the file's own, and the library's wall type of those layers, made when it has none. A project's IFCX file holds its library as the nodes a library file holds.

**Tech Stack:** Rust, `bim-core`, `bim-library`, the Revit reader crates; cargo with `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`, the shared `target/`.

**Spec:** `docs/superpowers/plans/2026-11-14-modeller-roadmap.md`, sub-project 6. Reference: Open BIM Studio `main`, `ui/properties/bim_type_layers.dl`, `ifc/bim_ifc_native.dl` 191-255 and 673-688, `ifc/bim_ifc.dl` 672-700 and 828.

## Global Constraints

- A build-up's layers run from the exterior face in, the exterior on the right walking from start to end. A file's wall with its exterior on the left is taken over with its ends changed round; it stands where it stood.
- Nothing the library has is changed: a material or type is reused or added, never rewritten.
- Modeller only; one branch; commits end with `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.

## Review Focus

1. Which side a file's wall has its exterior on: taken as the left of its location line unless the wall is flipped. Checked on one sample only. Task 1.
2. Layers that are not the wall's thickness together (within 1 mm): no build-up, the wall keeps its one thickness. Task 1.
3. A grip pressed on a file's wall that is turned round on take-over moves the end that was pressed. Task 1.
4. A door stays where it stands when the wall's start is dragged. Task 1.

## Not in this plan

A material's properties beyond its name and colour (thermal, structural). Layers of the floors, roofs and ceilings of a file: those are not taken over yet. The file's own properties of the wall on the new element.

---

### Task 1: A wall taken over keeps its layers

**Files:** Create `src/app/modeller/file_types.rs`; modify `src/model.rs` (`SourceWall::layers`, `exterior_left`, `SourceLayer`, `ProjectData::types`), `src/loaders/rvt.rs` (`source_walls`), `src/app/take_over.rs`, `src/app/grips.rs`, `src/app/edit.rs` (`with_points`), `src/export/ifcx.rs` and `src/loaders/ifcx.rs` (the library in a project file).

**Interfaces:** Produces `file_types::file_build_up(library, type_name, thickness, layers) -> Option<BuildUpDef>`, `file_types::turned_round(wall) -> PlanarDef`, `export::ifcx::LIBRARY`.

- [x] Tests: the layers become a wall type that is reused; layers of another thickness give none; a wall turned round stands where it stood; a layered wall of a file is taken over, saved as IFCX and read back with its layers and the library's materials.
- [x] Implement; whole suite; a wall of a sample file taken over; commit; push.

### Task 2: Layers in IFC4

**Files:** Modify `src/export/ifc.rs` (a material layer set per build-up, its usage per wall and slab), `src/loaders/ifc.rs` (layer sets read as properties and, for taking over, as layers).

- [ ] Test: a layered wall written as IFC4 and read back has the same layers, thicknesses and material names.
- [ ] Implement; whole suite; commit; push.
