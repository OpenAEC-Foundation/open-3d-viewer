# Modeller: walls between levels Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a wall, a standing system and a column stand on a level and reach either a height of their own or a level above (with an offset); moving a level moves what stands on it and stretches what reaches up to it, in one undo step.

**Architecture:** the kernel element gets an optional top constraint (`Element::top: Option<Top>`), kept in its IFCX definition. Absolute geometry stays in the shape; the app works the shape out from the levels (`src/app/modeller/tops.rs`) whenever a level moves or the constraint changes. Elements are rebuilt through one kernel function that keeps the constraint. The levels join the Modeller's history step.

**Tech Stack:** Rust; `bim-core`; iced 0.14. Cargo with `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`, the shared `target/`.

**Spec:** `docs/superpowers/plans/2026-11-14-modeller-roadmap.md`, sub-project 3. Reference: Open BIM Studio `main`, `elements/planar/records.dl` 17-45 (`levelId`, `baseOffset`, `height`, `topLevelId`), `bim/bim_model.dl` 2297-2318 (a level's elevation changes) and 2379-2391, 2512-2560 (top constraint; a typed height frees it).

## Global Constraints

- The base level is `Element::level` (exists); the base offset is the base's height above that level, not stored.
- A top is a level and an offset above it; with a top the height is not typed (it follows), and typing one frees the top.
- What cannot be made (a top at or below the base) is refused by the kernel and nothing changes.
- A file written before this opens with the same geometry (`top` defaults to none).
- Modeller only; one branch `claude/modeller-select-and-keys`; every text in every language; commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. A level is removed while walls reach up to it: their top is freed, their height kept. Task 2.
2. A level moved below the base of a wall that reaches up to it: refused, nothing moves. Task 2.
3. Undo of a level's move puts the level and the walls back together. Task 3.
4. A wall copied or moved keeps its top level. Task 1 (rebuilds keep it).
5. A beam (not vertical) has no top: its properties show none. Task 4.

---

### Task 1: The top in the kernel, kept through every rebuild

**Files:** `crates/bim-core/src/element.rs`; the rebuild sites in `src/app/{edit.rs, element_props.rs, grips.rs, openings.rs, transform.rs (2), modeller.rs (715)}`, `src/app/modeller/{levels.rs, shape_handles.rs}`.

**Interfaces:** Produces `pub struct Top { pub level: String, pub offset: f64 }` (serde), `Element::top: Option<Top>` (serde default, skipped when none), `Element::reshaped(&self, grid: &TileGrid, shape: Shape) -> Element` (same id, name, level and top, the shape given in project coordinates).

- [ ] Test `an_element_keeps_its_top_when_reshaped_and_reads_without_one` (element.rs): `reshaped` keeps id, name, level and top; an element serialized without `top` reads back with `None`; one with a top round-trips.
- [ ] Run to fail; implement; run to pass.
- [ ] Every rebuild site above uses `reshaped` (or sets `top` from the old element); test `a_wall_moved_keeps_its_top` (src/app/modeller/tops.rs, Task 2's file) moves a wall with a top through `begin_transform(Move)` and checks `top` is still set.
- [ ] Commit.

### Task 2: Tops follow the levels

**Files:** Create `src/app/modeller/tops.rs`; modify `src/app/modeller/levels.rs` (`set_level`), `src/app/levels.rs` (removing a level).

**Interfaces:** Produces `App::fit_tops(&self, model: &mut bim_core::Model, ids: Option<&[ElementId]>) -> Result<Vec<ElementId>, Refusal>` (gives the shapes of the elements with a top the height their top says: a wall's and a system's height, a vertical column's top end; `None`: all), `App::set_top(&mut self, id: ElementId, top: Option<Top>) -> Result<(), Refusal>`.

- [ ] Tests in `tops.rs`:
  - `a_wall_reaches_its_top_level_and_follows_it`: walls on Level 0 (0 m) with a top at Level 1 (3 m), offset 0 → height 3; Level 1 moved to 3.4 → height 3.4; Level 0 moved to 0.2 → base 0.2, height 3.2.
  - `a_top_offset_and_a_column`: top Level 1 offset −0.3 → height 2.7; a column on Level 0 with top Level 1 ends at 3.0.
  - `a_level_moved_below_a_base_is_refused`: Level 1 to −1 → refused, levels and walls as they were.
  - `removing_a_level_frees_the_tops_that_reach_it`: the added Level 1 removed → top `None`, height 3 kept.
- [ ] Run to fail; implement (`set_level` works on a copy: lift what stands on the level, then `fit_tops` for all; removing a level frees its tops first); run to pass; commit.

### Task 3: A level's move is one undo step

**Files:** `src/app/modeller.rs` (`Step::levels`), `src/app/undo.rs`, `src/document.rs` (`Document::set_levels(&mut self, levels: Vec<Level>)`).

- [ ] Test `undo_puts_the_level_and_its_walls_back` (tops.rs): move Level 1, Undo → level at 3 and wall height 3; Redo → 3.4 and 3.4.
- [ ] Run to fail; implement (the step holds the levels; restoring sets them); run to pass; commit.

### Task 4: Properties and the Wall tool

**Files:** `src/app/element_props.rs` (rows), `src/app/modeller/tops.rs` (the rows' widgets and messages), `src/app/modeller.rs` (`shape_for` / `add_element` of the Wall, Column and System tools), `locales/*.json`.

**Interfaces:** `Message::Top(TopMessage)` with `TopMessage::{Level(Option<String>), Offset(String), OffsetDone}`.

Rows for a wall, a standing system and a vertical column: Base level (the level's name, or "none"), Base offset (typed: the base moves), Top (a pick list: Free, then the levels above the base level), Top offset (typed, while a top is set). Height is shown, not typed, while a top is set; typing a height with no top as before.

- [ ] Tests (tops.rs): `the_properties_set_a_top_and_typing_a_height_frees_it` through the messages; `the_wall_tool_reaches_the_level_above` (a wall placed with no height typed on Level 0 of two gets top Level 1; on the highest level none).
- [ ] Run to fail; implement; translations; run the whole suite; commit.

### Task 5: The top grip takes a level or frees the top

**Files:** `src/app/modeller/shape_handles.rs`.

- [ ] Test `the_top_grip_takes_the_level_it_is_dropped_on_and_frees_the_top_elsewhere` (shape_handles.rs): dropped near Level 1 → top Level 1 offset 0; dropped at 3.5 m → top `None`, height 3.5.
- [ ] Run to fail; implement; run to pass; build the app, screenshot of a wall's properties with its top; commit; push.
