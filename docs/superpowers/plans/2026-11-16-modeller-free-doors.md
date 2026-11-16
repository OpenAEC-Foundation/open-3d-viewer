# Modeller: free-standing doors and windows Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a door or a window is an element of its own, not tied to a wall: it cuts an opening in every wall or timber frame wall it stands in, and stays when it is moved out or the wall goes.

**Architecture:** a point element with `PointDef::opening` (door or window): its place is the middle of its width at its foot, its direction its rotation, its size its width, leaf thickness and height. `bim_core::Model::standing_openings` finds, for a wall or a framed system, the openings of the doors and windows standing in it; the wall's body is made with them, so nothing is stored on the wall. The Door and Window tools (`src/app/openings.rs`) place such an element: on the wall aimed at, or free.

**Tech Stack:** Rust, `bim-core`; cargo with `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`, the shared `target/`.

**Spec:** `docs/superpowers/plans/2026-11-14-modeller-roadmap.md`, sub-project 8. Open BIM Studio differs here: its doors are always hosted.

## Global Constraints

- A door stands in a wall when its foot lies within the wall's thickness of the axis (and 10 mm), it faces along the wall, its width lies between the wall's ends, and its foot is not below the wall's base or above its top.
- A wall that cannot be made with a door's opening (too near its end, overlapping another) is made without it: the door never makes a wall fail.
- Modeller only; one branch; commits end with `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.

## Review Focus

1. A door moved from one wall into another: the first closes, the second opens.
2. A wall deleted leaves its doors and windows standing.
3. A curtain wall takes no door: one placed there stands free.

## Not in this plan

Openings of files written before this stay openings of their walls (holes without an element): the doors of a wall taken over from an opened file are that file's own elements, so making elements of them would show them twice. A door's own shape (frame, leaf, swing): it is drawn as a leaf of 40 mm. Door and window types in the library.

---

### Task 1: The element and the tools

**Files:** Modify `crates/bim-core/src/element.rs` (`PointDef::opening`, `OpeningKind`, the class), `crates/bim-core/src/model.rs` (`standing_openings`, `openings_of`, the body), `src/app/openings.rs` (`place_opening`), `src/export/ifc.rs` (the classes).

- [x] Tests: kernel (a door cuts the wall it stands in; moved, the first closes; square to the wall, beside it or past its end it cuts nothing; the wall removed, it stands); app (placed with the tool, moved with Move, placed free, left when its wall is deleted).
- [x] Implement; whole suite; commit; push.
