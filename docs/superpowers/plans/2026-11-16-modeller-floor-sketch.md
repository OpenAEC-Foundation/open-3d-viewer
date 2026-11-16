# Modeller: floors by sketch and by handle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a floor is shaped two ways: finished, by handles on its edges, its top and its underside, in the floor plan, in 3D and in a section; and in a sketch of its outline, where lines are added and trimmed.

**Architecture:** two Modeller modules. `src/app/modeller/slab_handles.rs`: a selected slab shows a grip on every edge and on its top and underside; a drag runs along one line (the edge's normal in plan, or the vertical) and is found as the point of that line nearest the cursor's ray, so it works from above and from the side alike. `src/app/modeller/sketch.rs`: Tab on a slab opens its outline as line segments; segments are drawn and trimmed; finishing needs one closed loop, which a pure function finds.

**Tech Stack:** Rust, iced 0.14, `bim-core`; cargo with `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`, the shared `target/`.

**Spec:** `docs/superpowers/plans/2026-11-14-modeller-roadmap.md`, sub-project 5. Reference: Open BIM Studio `main`, `ui/editor/bim_boundary_sketch.dl` (the sketch: lines, arcs with a bulge, trim; one closed loop, else refused: 197-200) and `ui/editor/bim_editor_core.dl` 5882-6033 (a slab's edge midpoint pushes that side, a corner drags the corner).

## Global Constraints

- A drag is rounded to 10 mm, as other moves are; what the kernel cannot make leaves the floor as it was and says why.
- Every drag and every finished sketch is one undo step.
- Modeller only; one branch; every text translated; commits end with `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.

## Review Focus

1. An edge pushed past its neighbours' far ends (the outline would cross itself): refused by the kernel, nothing changes. Task 1.
2. A slab seen exactly edge-on in a section: the grips of the edges the view looks along lie on one point; the nearest to the camera is taken. Task 1.
3. The underside dragged above the top: refused. Task 1.
4. A sketch with a gap, a branch or two loops cannot be finished and says so; Esc leaves the floor unchanged. Task 2.
5. A trim click on a stretch that no other line crosses takes the whole segment. Task 2.

## Not in this plan

Arcs (a segment with a bulge, kept by the kernel) and floor openings by two clicks: the next plan of this sub-project. Roofs: once a roof is an element.

---

### Task 1: Handles on the finished floor

**Files:** Create `src/app/modeller/slab_handles.rs`; modify `src/app/edit.rs` (`EditOverlay::outline`, the overlay's events when a slab drag is on), `src/app/grips.rs` (the overlay and press of a selected slab), `src/app/modeller.rs` (`Modeller::slab_drag`).

**Interfaces:** Produces `pub enum SlabHandle { Edge(usize), Top, Under }`, `pub struct SlabDrag { pub element: ElementId, pub handle: SlabHandle, pub along: f64 }`, `App::slab_grips(&self, id) -> Vec<(SlabHandle, DVec3)>`, `slab_handles::pushed(shape: &Shape, handle: SlabHandle, along: f64) -> Option<Shape>`.

`pushed`: an edge moves `along` its outward normal, its two corners sliding along the neighbouring edges' lines (parallel neighbours: the corners move with the edge); Top moves the top and keeps the underside (the thickness changes); Under moves the underside (the thickness changes, the top stays).

- [ ] Tests in `slab_handles.rs`:
  - `pushed_moves_an_edge_and_its_neighbours_follow`: a 4 × 3 rectangle, edge 1 (x = 4) pushed 1 m → 5 × 3; an L shape, an inner edge pushed, its corners slide along the neighbours.
  - `top_and_underside_change_the_thickness`.
  - `a_selected_slab_has_a_grip_on_every_edge_and_dragging_one_moves_the_edge` (from above).
  - `in_a_section_the_side_of_a_floor_is_dragged` (camera from the front: the grip of the edge at x = 4 dragged to x = 5).
  - `the_underside_dragged_above_the_top_is_refused`.
- [ ] Run to fail; implement; run to pass; whole suite; commit.

### Task 2: The sketch of a floor's outline

**Files:** Create `src/app/modeller/sketch.rs`; modify `src/app/edit.rs` (Tab on a slab opens the sketch), `src/shell.rs` (the sketch's keys), `src/app.rs` (messages, status line), `locales/*.json`.

**Interfaces:** Produces `pub struct Sketch { pub element: ElementId, pub top: f64, pub segments: Vec<[DVec2; 2]>, pub tool: SketchTool, pub first: Option<DVec2> }`, `pub enum SketchTool { Pick, Line, Rectangle, Trim }`, `sketch::closed_loop(segments: &[[DVec2; 2]]) -> Result<Vec<DVec2>, SketchError>` (`Gap`, `Branch`, `TwoLoops`, `Empty`), `sketch::trimmed(segments, at: DVec2) -> Vec<[DVec2; 2]>`.

Keys in the sketch: L line (chained clicks), R rectangle (two clicks), T trim (a click takes the stretch of a segment between its two nearest crossings with other segments, or the whole segment), Del removes the picked segment, Tab finishes (one step), Esc: a point clicked goes first, then the sketch leaves with the floor unchanged.

- [ ] Tests in `sketch.rs`: `closed_loop` on a rectangle in any segment order and direction, with a gap, a branch, two loops; `trimmed` on two crossing lines and on a lone segment; an app test: Tab on a slab, R with two clicks beside it, T on the stretches inside, Tab → the slab is the union outline; Esc leaves it unchanged.
- [ ] Run to fail; implement; run to pass; translations; whole suite; screenshot of the sketch; commit; push.
