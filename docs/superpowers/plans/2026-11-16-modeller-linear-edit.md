# Modeller: edit mode for linear elements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** in edit mode a grabbed point is moved by a typed distance, and a beam or column shows its ends and its length in the properties.

**Architecture:** edit mode (`src/app/edit.rs`) already shows a member's two ends, grabs them with G and holds the move to X, Y or Z. A new Modeller module `src/app/modeller/typed_move.rs` adds the typed distance: the grab keeps the text typed, which gives the move in place of the cursor. The shell sends digits to edit mode while a grab is under way.

**Tech Stack:** Rust, iced 0.14; cargo with `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`, the shared `target/`.

**Spec:** `docs/superpowers/plans/2026-11-14-modeller-roadmap.md`, sub-project 4. Reference: Open BIM Studio `main`, `ui/editor/bim_editor_core.dl` 8615-8659 (during a grab: X / Y / Z lock an axis, digits type a distance, Enter places, Esc cancels) and 7922-8044 (a member's two ends).

## Global Constraints

- A typed distance is in metres, a point or a comma, a minus allowed.
- It runs along the locked axis; with no axis locked, along the member away from its other end (a member with one end picked). Where neither says a direction, typing changes nothing.
- While a distance is typed the cursor does not move the points. Backspace takes a character back; Enter puts the move down as one undo step; Esc drops it.
- Modeller only; one branch; every text translated; commits end with `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.

## Review Focus

1. Enter with nothing typed, or with text that is no number, must not put the move down at a wrong place. Task 1.
2. Digits typed while no grab is under way must stay the app's shortcuts (0–3 are camera views). Task 1.
3. A typed distance on a wall's end (edit mode, no axis): nothing moves, and the status line does not claim it does. Task 1.
4. The top grip's drag with a distance typed changes the height by that distance. Task 1.

## Not in this plan

Taking over a beam or column of an opened file. The file reader gives columns but no beam's axis or profile; that is work in the reader first. It stays an open item of the roadmap.

---

### Task 1: A typed distance moves the grabbed points

**Files:** Create `src/app/modeller/typed_move.rs`; modify `src/app/edit.rs` (`Grab::typed: String`, `edit_key`, `grab_to`, `put_down` visibility), `src/app/modeller/shape_handles.rs` (the `Grab` literal), `src/app.rs` (Enter and Backspace while typing, the status line), `src/shell.rs` (digits to edit mode during a grab).

**Interfaces:** Produces `App::edit_type(&mut self, c: char)`, `App::edit_backspace(&mut self) -> bool`, `App::edit_enter(&mut self) -> bool`, `App::typed_distance(&self) -> Option<&str>`.

- [ ] Tests in `typed_move.rs`:
  - `a_typed_distance_lifts_a_beams_end_in_one_step`: a beam, Tab, press its end, G, Z, `0`, `.`, `5`, Enter → the end is 0.5 m higher, edit mode stays, one Undo puts it back.
  - `with_no_axis_a_member_grows_along_itself`: a beam 4 m long, its end picked, G, `1`, Enter → 5 m long, in line.
  - `backspace_takes_a_character_back_and_enter_needs_a_number`: `1`, `2`, Backspace → "1"; `-` alone and Enter → nothing put down, the grab still on.
  - `typing_on_a_walls_end_without_an_axis_moves_nothing`.
  - `the_top_grip_takes_a_typed_distance` (press the top grip, `0`, `.`, `4`, Enter → 0.4 m higher).
  - in `shell::tests`: a digit with no grab stays a camera shortcut; with a grab it goes to edit mode.
- [ ] Run to fail; implement; run to pass; commit.

### Task 2: A member's ends and length in the properties

**Files:** `src/app/element_props.rs`.

- [ ] Test `a_member_shows_its_ends_and_its_length` (element_props.rs tests): a selected beam's properties show "Start", "End" (x; y; z in metres) and "Length".
- [ ] Run to fail; implement (read-only rows for `Shape::Linear`); translations; whole suite; commit; push.
