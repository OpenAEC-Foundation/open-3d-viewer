# Modeller: temporary dimensions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a selected wall shows how far it stands from what runs beside it, and a value typed there moves the wall: the first part of sub-project 9.

**Architecture:** `src/app/modeller/temp_dims.rs`. For the one wall or standing system with grips, the nearest parallel line on either side is found: the axis of another wall or system its middle lies beside, or a grid line. A canvas draws the dimension lines; a text field on each holds the distance. A value entered moves the wall as its middle grip dragged that far, so the walls joined to it follow.

**Tech Stack:** Rust, iced 0.14, `bim-core`; cargo with `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`, the shared `target/`.

**Spec:** `docs/superpowers/plans/2026-11-14-modeller-roadmap.md`, sub-project 9. The reference modeller has no temporary dimensions; the behaviour follows the wish (as in Revit).

## Decisions

- The selected wall moves; what it is measured to stays.
- Distances are between axes (centre lines), in metres with three decimals.
- A text that is no number, or a value below zero, moves nothing.

## Review Focus

1. A wall 3.00 m from a grid line is put at 3.60 m by typing, in one undo step.
2. The walls joined to the moved wall follow, as with its middle grip.
3. Esc while a value is typed drops the typing, not the selection.

## Not in this plan

Dimensions between two picked features that follow their elements (references, chains, a marked lost reference), and typing in such a dimension: the second part of sub-project 9. Temporary dimensions between faces, to walls that are not parallel, and for slabs, columns and beams.

---

### Task 1: Temporary dimensions of a wall

**Files:** Create `src/app/modeller/temp_dims.rs`; modify `src/app/modeller.rs` (`temp_dim`), `src/app/grips.rs` (`gripped` for the Modeller's modules), `src/app.rs` (the message, Esc, the layers over the viewport).

- [x] Tests: the Done-when of the roadmap; a wall between two others has a dimension to each and the joined walls follow; Esc and a text that is no number.
- [x] Implement; whole suite; commit; push; checked in the running app over the MCP.
