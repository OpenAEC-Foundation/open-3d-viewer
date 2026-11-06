# Phase 0 and 1: Frame Budget and Selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure the 120 fps frame budget with a throughput benchmark, then add multi-select, element hide/isolate/show, box selection, select-by-value, and undo/redo of visibility, without re-uploading geometry.

**Architecture:** The per-unit texture of the exploded view (one unit per element, `Rgba32Float`, 1024 units per row) becomes the general per-element channel: `xyz` stays the explode offset, `w` carries flags (hidden, selected). Hiding and highlighting become one texture write instead of a geometry re-upload and one draw call per range. The app keeps a `Selection` of pickables, a set of hidden `UnitKey`s that stay stable when a model is added, and a `History` of visibility snapshots.

**Tech Stack:** Rust 1.96, iced 0.14, wgpu (iced's, WebGPU and WebGL2), WGSL.

**Spec:** [`docs/superpowers/specs/2026-11-06-bim-modeller-design.md`](../specs/2026-11-06-bim-modeller-design.md), phases 0 and 1.

## Global Constraints

- The 120 fps criterion: at least 120 submitted fps, p95 frame time at most 8.33 ms, frames over 16.67 ms counted. It is hard on the reference desktop PC with a discrete GPU. On integrated graphics the rule is no regression against the Phase 0 baseline.
- Camera-only frames never wait for selection or visibility work; that work runs in `update`, between frames.
- WebGL2 fallback: no storage buffers. Per-unit data stays in the `Rgba32Float` texture, `OFFSETS_WIDTH = 1024` units per row (`src/viewport/render.rs:992`, `scene.wgsl:26`).
- The browser build keeps compiling: `cargo check --release --locked --target wasm32-unknown-unknown --lib --bin open-3d-viewer`. Everything in this plan also works in the browser.
- Tests that must pass before each commit: `cargo test --release --locked` and `cargo test --release --locked -p openaec-ui --lib`. Tests that need a GPU are `#[ignore]`d, as `src/shell.rs:1046` does.
- Every command goes into the one command list in `src/shell.rs` (used by the ribbon, the title bar and the keyboard), with a Dutch and an English label through `l.text(nl, en)`. The shell tests check names and duplicate shortcuts.
- `README.md` and `README.nl.md` change together. `python scripts/build_readme_images.py --check` stays green.
- No names of competing commercial software in code, comments, docs or commit messages. Content in English.
- Commit messages follow the repository's style (a plain sentence such as "Hide and isolate elements") and end with the `Co-Authored-By` trailer the session prescribes.
- Work on a branch from `main` (`feat/selection-visibility`), one pull request for Phase 0 and one for Phase 1.

## Deviations from the spec

- **Ctrl-click instead of Shift-click.** In the viewport a Shift + left drag already pans (`src/viewport/mod.rs:3-5`), so Shift cannot also add to the selection. Ctrl-click adds or removes, and Ctrl + drag draws a box.
- **No context menu yet.** The spec lists "the context of the selection" as one way to hide. A right-button drag pans today, so a context menu needs its own interaction design. Here, hiding goes through the ribbon, shortcuts and the MCP server.

## Review Focus

- **More units than WebGL2 can hold** (over 2 048 rows × 1 024 = 2 097 152 units): hiding or selecting must not crash; flags beyond the texture are ignored. Test in Task 3 (`unit_state_is_capped_to_texture_limit`).
- **Formats without elements** (OBJ, STL): a click selects the whole layer, and Hide must hide that layer's geometry. Test in Task 6 (`hide_works_for_formats_without_elements`).
- **A hidden element in front of another:** a click goes through it to the element behind. Test in Task 6 (`pick_passes_through_hidden_element`).
- **Adding a model after hiding:** what was hidden stays hidden, and undo still restores it. Tests in Task 6 (`hidden_survive_add_model`) and Task 7 (`undo_survives_add_model`).
- **The exploded view with hidden elements:** a hidden element stays hidden while it moves apart. Test in Task 6 (`explode_and_hidden_combine`).

---

## Phase 0: the frame budget

### Task 1: Throughput mode for `--bench`

**Files:**
- Modify: `src/bench.rs` (whole file; today it measures latency, waiting for the GPU after every frame at line 116)
- Modify: `src/desktop.rs:105-106` (argument parsing)
- Modify: `README.md`, `README.nl.md` (command line block in *Quick start*)

**Interfaces:**
- Produces:
  - `pub struct FrameStats { pub frames: u32, pub fps: f64, pub p95_ms: f64, pub max_ms: f64, pub over_budget: u32 }`
  - `pub fn frame_stats(durations_ms: &[f64], wall_s: f64) -> FrameStats`: p95 by nearest rank on the sorted durations; `over_budget` counts durations over 16.67 ms; `fps = frames / wall_s`.
  - `impl FrameStats { pub fn passes(&self) -> bool }`: `fps >= 120.0 && p95_ms <= 8.33`.
  - `pub enum Scenario { Orbit, Pan, Zoom }` with `fn step(self, camera: &mut Camera, frame: u32, height: f32)`, using `Camera::orbit`, `Camera::pan` and `Camera::zoom` (`src/viewport/camera.rs:118-133`). Pan and zoom reverse direction every 100 frames so the model stays in view.
  - `pub fn run(file: &Path, frames: u32, throughput: bool) -> Result<(), String>`. Without `throughput` it behaves as today.

- [ ] **Step 1: Write the failing tests** in `src/bench.rs`

```rust
#[test]
fn frame_stats_counts_frames_over_budget() {
    let s = frame_stats(&[5.0, 5.0, 20.0, 8.0], 0.038);
    assert_eq!(s.frames, 4);
    assert_eq!(s.over_budget, 1);
    assert_eq!(s.max_ms, 20.0);
    assert_eq!(s.p95_ms, 20.0);
    assert!((s.fps - 105.26).abs() < 0.01);
}

#[test]
fn passes_needs_fps_and_p95() {
    let s = |fps, p95_ms| FrameStats { frames: 300, fps, p95_ms, max_ms: p95_ms, over_budget: 0 };
    assert!(s(130.0, 8.0).passes());
    assert!(!s(130.0, 9.0).passes());
    assert!(!s(110.0, 7.0).passes());
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --release --locked bench::`
Expected: FAIL, `frame_stats` and `FrameStats` not found.

- [ ] **Step 3: Implement `FrameStats`, `frame_stats`, `Scenario` and the throughput loop in `src/bench.rs`**

The throughput loop keeps at most two frames in flight, as a swap chain would:
- after each `queue.submit`, register `queue.on_submitted_work_done` to push the completion `Instant` into an `Arc<Mutex<Vec<Instant>>>`;
- before submitting frame `i`, call `device.poll(PollType::wait_indefinitely())` only while `i - completed >= 2`;
- a frame's duration is the time between two consecutive completions;
- `wall_s` runs from the first submit to the last completion.

Per scenario, print exactly:
`{scenario} @ 1920x1080 4xMSAA: {frames} frames, {fps:.0} fps, p95 {p95:.2} ms, max {max:.2} ms, {over} over 16.67 ms: {PASS|FAIL}`.
After all scenarios, print `120 fps criterion: PASS` or `120 fps criterion: FAIL`.

- [ ] **Step 4: Add `--throughput` in `src/desktop.rs`** next to `--frames` and pass it to `bench::run`; add `[--throughput]` to the `--bench` line of the command-line block in both READMEs.

- [ ] **Step 5: Run the tests and one benchmark**

Run: `cargo test --release --locked bench::` → PASS.
Run: `cargo run --release -- --bench test-models/test-kavel.skp --throughput --frames 300` → three scenario lines and a criterion line.

- [ ] **Step 6: Commit**: "Measure frame throughput with --bench --throughput".

### Task 2: A synthetic reference model and the baseline

**Files:**
- Modify: `src/bench.rs`
- Create: `docs/performance/baseline.md`

**Interfaces:**
- Produces: `pub fn synthetic(n: usize) -> Model`. It builds `n` boxes of 1 m with 0.5 m gaps on a square grid, each box its own element (ids `1..=n`, 12 triangles), 10 000 elements per layer (`Boxes 1`, `Boxes 2`, ...), properties `{"Name":"Box {i}"}`. `run` accepts `synthetic:N` as the file.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn synthetic_has_one_element_per_box() {
    let m = synthetic(25);
    let ids: std::collections::BTreeSet<u64> = m.layers.iter().flat_map(|l| l.elements.iter().map(|e| e.0)).collect();
    assert_eq!(ids, (1..=25).collect());
    assert_eq!(m.triangle_count(), 25 * 12);
    assert_eq!(m.element_props(7).first().map(|(k, v)| (k.as_str(), v.as_str())), Some(("Name", "Box 7")));
}
```

- [ ] **Step 2: Run it to see it fail**: `cargo test --release --locked synthetic_has_one_element_per_box` → FAIL.
- [ ] **Step 3: Implement `synthetic`** and the `synthetic:N` file argument.
- [ ] **Step 4: Run it to see it pass**: same command → PASS.
- [ ] **Step 5: Measure the baseline** on this machine:
  - `cargo run --release -- --bench synthetic:100000 --throughput --frames 300`
  - `cargo run --release -- --bench test-models/test-kavel.skp --throughput --frames 300`
  - any large IFC or `.rvt` sample available locally (name it in the table, do not commit it).
- [ ] **Step 6: Write `docs/performance/baseline.md`**: one table with date, commit, adapter and backend (printed by the bench), whether the GPU is discrete or integrated, model, scenario, fps, p95, max and frames over budget. Add one paragraph on how to rerun it.
- [ ] **Step 7: Commit**: "Record the frame budget baseline".
- [ ] **Step 8: Gate.**
  - If this machine has no discrete GPU, record the table as the integrated-graphics baseline, and ask the project owner to run the same commands on the reference PC before Phase 1 is merged.
  - If any scenario FAILS on the reference PC, stop here and report the numbers: the renderer work to reach 120 fps gets its own plan, as the spec's Phase 0 requires.

---

## Phase 1: selection and visibility

### Task 3: Unit flags in the shader

**Files:**
- Create: `src/units.rs` (registered in `src/lib.rs`)
- Modify: `src/viewport/scene.wgsl:23-56` (`place`, `vs_main`, `VertexOut`), `scene.wgsl:125-154` (`fs_lit`, `fs_glass`), and `vs_face` (cut fill)
- Modify: `src/viewport/render.rs` (doc comments of `offsets_texture` and `write_offsets`; an ignored GPU test)
- Modify: `src/explode.rs` (module doc: units also carry flags)

**Interfaces:**
- Produces in `src/units.rs`:
  - `pub const UNIT_HIDDEN: u8 = 1; pub const UNIT_SELECTED: u8 = 2;`
  - `pub const MAX_UNITS: usize = 2048 * 1024;`
  - `pub fn unit_state(offsets: &[[f32; 4]], flags: &[u8]) -> Vec<[f32; 4]>`: one entry per unit, `max(offsets.len(), flags.len())` long but at most `MAX_UNITS`. `xyz` comes from `offsets` (zero where missing), `w` is the flag byte as `f32`. Unit 0 (geometry outside any unit) never gets flags.
- Shader contract: `let flags = u32(state.w + 0.5)`. With bit 1 (hidden), `vs_main` and `vs_face` put the vertex at `vec4(0.0, 0.0, 2.0, 1.0)`, beyond the far plane, so the triangle is clipped: no faces, edges or cut fill. With bit 2 (selected), `VertexOut` passes `@location(3) @interpolate(flat) flags: u32`, and `fs_lit` and `fs_glass` mix the lit colour towards the amber exactly as `fs_highlight` does today.

- [ ] **Step 1: Write the failing tests** in `src/units.rs`

```rust
#[test]
fn unit_state_puts_flags_in_w() {
    let s = unit_state(&[[0.0; 4], [1.0, 2.0, 3.0, 0.0]], &[0, UNIT_HIDDEN | UNIT_SELECTED, UNIT_SELECTED]);
    assert_eq!(s.len(), 3);
    assert_eq!(s[1], [1.0, 2.0, 3.0, 3.0]);
    assert_eq!(s[2], [0.0, 0.0, 0.0, 2.0]);
}

#[test]
fn unit_zero_never_gets_flags() {
    assert_eq!(unit_state(&[], &[UNIT_HIDDEN])[0][3], 0.0);
}

#[test]
fn unit_state_is_capped_to_texture_limit() {
    assert_eq!(unit_state(&[], &vec![UNIT_HIDDEN; MAX_UNITS + 10]).len(), MAX_UNITS);
}
```

- [ ] **Step 2: Run them to see them fail**: `cargo test --release --locked units::` → FAIL.
- [ ] **Step 3: Implement `unit_state`** and the shader contract above. Keep the highlight pipeline for now; Task 4 removes it.
- [ ] **Step 4: Add an ignored GPU test** `hidden_unit_is_not_drawn` in `src/viewport/render.rs`.
  - Render the two squares of `app.rs`'s `two_squares()` offscreen from above (the setup of `bench.rs:41-61`).
  - Give the unit of the upper square `UNIT_HIDDEN`, read back the centre pixel (as `bench::write_target` does), and assert that it is the colour of the lower square, not the upper one.
  - Mark it `#[ignore = "needs a GPU"]`.
- [ ] **Step 5: Run the tests**
  - `cargo test --release --locked units::` → PASS.
  - `cargo test --release --locked hidden_unit_is_not_drawn -- --ignored` → PASS on a machine with a GPU.
  - The wasm check from the Global Constraints passes.
- [ ] **Step 6: Commit**: "Carry hidden and selected flags per element on the GPU".

### Task 4: Multi-select with Ctrl-click, drawn through the flags

**Files:**
- Create: `src/selection.rs`
- Modify: `src/app.rs`:
  - the fields `selected` and `selection` (99-102) become caches of `sel`;
  - `select` (1277-1284), `pick` (1236-1273), `set_scene` (703-735), `Message::Escape`;
  - the properties pane header (1694-1730);
  - the `ScenePrimitive` built in `view` (2040-2045).
- Modify: `src/viewport/mod.rs` (track modifiers, `Navigate::Pick` gets `add: bool`)
- Modify: `src/viewport/render.rs` (remove `ScenePrimitive::selection`, the highlight pipeline `fs_highlight` and the pass at 866-877; `offsets` now carries the unit state)
- Modify: `src/explode.rs` (accessors below)
- Modify: `src/remote.rs:810-839` (`remote_select`), `src/app/schedules.rs:100-113` (row select), `src/bench.rs` (primitive construction)

**Interfaces:**
- Consumes: `unit_state`, `UNIT_SELECTED` (Task 3).
- Produces in `src/selection.rs`:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub enum Pickable { Element(u64), Layer(usize) }`. `Layer` is only for layers without elements.
  - `#[derive(Debug, Clone, Default)] pub struct Selection`, in pick order, without duplicates, with:
    - `set(&mut self, item: Option<Pickable>)`, `toggle(&mut self, item: Pickable)`, `extend(&mut self, items: impl IntoIterator<Item = Pickable>)`, `clear(&mut self)`;
    - `items(&self) -> &[Pickable]`, `len`, `is_empty`, `contains(&self, item: Pickable) -> bool`;
    - `primary(&self) -> Option<Pickable>` (the first item);
    - `ranges(&self, model: &Model) -> Vec<(usize, u32, u32)>`.
- Produces in `src/explode.rs`:
  - `pub fn element_unit(&self, id: u64) -> Option<usize>`;
  - `pub fn layer_rest_unit(&self, layer: usize) -> Option<usize>`;
  - `pub enum UnitOwner { Element(u64), LayerRest(usize) }` and `pub fn owner(&self, unit: usize) -> Option<UnitOwner>`;
  - `pub fn unit_bounds(&self, unit: usize) -> Bounds` (at rest).
- Produces in `App`:
  - `pub sel: Selection`;
  - `pub units: Arc<Vec<[f32; 4]>>` (what the renderer gets; `explode_offsets` stays for the explode maths);
  - `fn selection_changed(&mut self)`, which recomputes `selected` (primary: `Element(id)` gives the first layer of its ranges and `Some(id)`, `Layer(l)` gives `(l, None)`), `selection` (`sel.ranges`) and `units`;
  - `fn update_units(&mut self)`, called by `selection_changed` and at the end of `update_explode`.
- `Navigate::Pick { x, y, width, height, add }`: `add` is true while Ctrl is held. `pick` casts as today and hands the result to `pub(crate) fn click(&mut self, hit: Option<Pickable>, add: bool)`:
  - without `add`, it does `sel.set(hit)`, so empty space clears;
  - with `add`, it does `sel.toggle(hit)` on a hit and keeps the selection on empty space.
- Properties pane: with more than one item, a first line "{n} elementen geselecteerd" / "{n} elements selected", then the primary's properties as today.

- [ ] **Step 1: Write the failing tests**

In `src/selection.rs`:

```rust
#[test]
fn toggle_removes_and_keeps_order() {
    let mut s = Selection::default();
    s.extend([Pickable::Element(1), Pickable::Element(2), Pickable::Element(3)]);
    s.toggle(Pickable::Element(2));
    assert_eq!(s.items(), &[Pickable::Element(1), Pickable::Element(3)]);
    s.toggle(Pickable::Element(2));
    assert_eq!(s.items().last(), Some(&Pickable::Element(2)));
}

#[test]
fn extend_skips_duplicates() {
    let mut s = Selection::default();
    s.extend([Pickable::Element(1), Pickable::Element(1)]);
    assert_eq!(s.len(), 1);
}
```

In `src/app.rs` tests, with `two_squares()` (element 10 at z = 0, element 20 at z = 1):
- `ctrl_click_adds_and_removes`: `click(Some(Element(20)), false)`, then `click(Some(Element(10)), true)`, gives `sel.len() == 2` and `sel.primary() == Some(Pickable::Element(20))`; `click(Some(Element(10)), true)` again gives `sel.len() == 1`.
- `plain_click_on_empty_space_clears_but_ctrl_click_keeps`: `click(None, true)` keeps the selection, and `click(None, false)` clears it.
- `ctrl_pick_reaches_click_with_add`: a `Navigate::Pick { add: true, .. }` from above at the squares' middle (the pick setup of the existing test at `app.rs:2979`) adds 20 to a selection that already holds 10.
- `selected_units_carry_the_selected_flag`: after selecting 20, the `w` of `app.units` at `explode_index.element_unit(20)` has `UNIT_SELECTED` set, and the `w` at the unit of 10 is 0.

- [ ] **Step 2: Run them to see them fail**: `cargo test --release --locked selection::` and `cargo test --release --locked app::tests::ctrl_click` → FAIL.
- [ ] **Step 3: Implement** `Selection`, the explode accessors, `selection_changed`, `update_units`, Ctrl tracking in the viewport, and the pane header. Switch every writer of `selected`/`selection` to `sel` plus `selection_changed`: `select`, `remote_select`, the schedule rows, `Escape` and `set_scene`. Remove the highlight pass.
- [ ] **Step 4: Run all tests**: `cargo test --release --locked` → PASS. The remote `select` tests still pass unchanged.
- [ ] **Step 5: Commit**: "Select several elements with Ctrl-click".

### Task 5: Element keys that survive Add

**Files:**
- Modify: `src/model.rs` (`Model`, `combine` at 339-385)
- Modify: `src/units.rs`
- Modify: `src/app.rs` (`add_model` at 669-679)

**Interfaces:**
- Produces in `src/model.rs`:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)] pub struct ElementKey { pub model: u32, pub source: u64 }`;
  - `Model.keys: HashMap<u64, ElementKey>` and `Model.ids: HashMap<ElementKey, u64>`, filled by `combine` (the index of the model in its argument slice, and the id before remapping), empty for a single model;
  - `pub fn element_key(&self, id: u64) -> ElementKey`: `ElementKey { model: 0, source: id }` when `keys` is empty;
  - `pub fn element_with_key(&self, key: ElementKey) -> Option<u64>`.
- Produces in `src/units.rs`: `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub enum UnitKey { Element(ElementKey), LayerRest { model: u32, layer: u32 } }`. `layer` is the layer's index within its own model, which `combine` keeps.
- Produces in `App`: `fn unit_key(&self, owner: UnitOwner) -> UnitKey`, which maps a combined layer index to `(model, layer)` through `ModelEntry::layers`; and `fn unit_of_key(&self, key: UnitKey) -> Option<usize>`.

- [ ] **Step 1: Write the failing tests**
  - `model.rs`: `combine_keeps_source_ids_as_keys`. Two models that both have element 7 give two different combined ids, with keys `(0, 7)` and `(1, 7)`.
  - `model.rs`: `element_with_key_inverts_element_key`, for every element of the combined model.
  - `model.rs`: `single_model_keys_are_identity`. In a model straight from `Model::new`, `element_key(42) == ElementKey { model: 0, source: 42 }` and `element_with_key` of that key is `Some(42)`.
  - `app.rs`: `keys_survive_add_model`, using the `source(name, x)` helper at `app.rs:2770`. Take the key of an element after `set_model(a)`, then `add_model(b)`; `element_with_key(key)` gives an id whose `element_props` equal the original ones.
- [ ] **Step 2: Run them to see them fail** → FAIL.
- [ ] **Step 3: Implement** the keys in `combine`, `element_key`, `element_with_key`, `UnitKey`, `unit_key` and `unit_of_key`.
- [ ] **Step 4: Run all tests** → PASS.
- [ ] **Step 5: Commit**: "Keep element keys stable when a model is added".

### Task 6: Hide, isolate and show all

**Files:**
- Modify: `src/app.rs`:
  - `Message` gets `HideSelection`, `IsolateSelection` and `ShowAllElements`;
  - a new field `pub hidden: Arc<HashSet<UnitKey>>`;
  - `update_units` sets `UNIT_HIDDEN`;
  - `pick_triangle` (2279-2337) gets a parameter `hidden: &dyn Fn(usize, u32) -> bool` (layer, a vertex of the triangle), and `cast` passes the hidden units;
  - `measure_hover` and edge snapping skip the triangles of hidden units;
  - `add_model` keeps `hidden`, and `set_model` clears it.
- Modify: `src/shell.rs`: the commands, and a ribbon group "Selectie" / "Selection" after "Camera".

**Interfaces:**
- Consumes: `Selection`, `UnitKey`, `unit_key`, `unit_of_key`, `UnitOwner` (Tasks 4 and 5).
- Semantics:
  - **Hide** adds the keys of the selected units to `hidden` and clears the selection. `Pickable::Layer(l)` hides `LayerRest` of `l`.
  - **Isolate** sets `hidden` to the keys of every unit of every model except the selected ones, and keeps the selection.
  - **Show all** empties `hidden`. Layer and model toggles are untouched.
  - `Escape` never changes `hidden`.
- Commands:

| id | label (nl / en) | icon | shortcut | enabled |
|---|---|---|---|---|
| `object.hide` | Verbergen / Hide | `object.hide` | `key('h')` | selection not empty |
| `object.isolate` | Isoleren / Isolate | `object.show` (stand-in) | `key('i')` | selection not empty |
| `object.show-all` | Alles tonen / Show all | `object.show` | `Shortcut::ctrl_shift('h')` | `hidden` not empty |

- [ ] **Step 1: Write the failing tests** in `src/app.rs`, with `two_squares()`:
  - `hide_hides_selected_units_and_clears_selection`: select 20 and hide it. The unit of 20 has `UNIT_HIDDEN` and `sel` is empty.
  - `isolate_hides_everything_else`: select 20 and isolate. The unit of 10 has `UNIT_HIDDEN`, 20 does not, and 20 is still selected.
  - `show_all_clears_hidden`.
  - `pick_passes_through_hidden_element`: hide 20, which lies above 10. A pick from above at the same x, y selects 10.
  - `hide_works_for_formats_without_elements`: a layer without `elements`, selected by a click, then hidden. Its `layer_rest_unit` has `UNIT_HIDDEN`.
  - `hidden_survive_add_model`: hide an element of model a and `add_model(b)`. That element's unit (found through `unit_of_key`) still has `UNIT_HIDDEN`.
  - `explode_and_hidden_combine`: explode on (`Message::ToggleExplode`, `Message::Explode(0, 50.0)`), then hide 20. Its `units` entry has a non-zero `x` offset and `UNIT_HIDDEN` in `w`.
- [ ] **Step 2: Run them to see them fail** → FAIL.
- [ ] **Step 3: Implement** the messages, the semantics, the hidden-aware picking and snapping, and the commands with the ribbon group.
- [ ] **Step 4: Run all tests**: `cargo test --release --locked` → PASS, the shell's shortcut and screen-reader tests included.
- [ ] **Step 5: Commit**: "Hide and isolate elements".

### Task 7: Undo and redo of visibility

**Files:**
- Create: `src/history.rs`
- Modify: `src/app.rs`:
  - `ViewState`, `history`;
  - recording in `Message::Layer`, `AllLayers`, `LayerGroup`, `ModelVisible`, `HideSelection`, `IsolateSelection`, `ShowAllElements`;
  - `Message::Undo` and `Message::Redo`.
- Modify: `src/shell.rs` (commands)

**Interfaces:**
- Produces in `src/history.rs`, where `History` keeps all entries (no limit):

```rust
pub struct History<T> { /* undo and redo stacks */ }
impl<T> History<T> {
    pub fn new() -> Self;
    /// The state before a change; clears redo.
    pub fn record(&mut self, before: T);
    /// The state to restore, given the current one (which goes to redo).
    pub fn undo(&mut self, current: T) -> Option<T>;
    pub fn redo(&mut self, current: T) -> Option<T>;
    pub fn can_undo(&self) -> bool;
    pub fn can_redo(&self) -> bool;
    pub fn clear(&mut self);
}
```

- Produces in `App`:
  - `#[derive(Debug, Clone, PartialEq)] pub struct ViewState { pub hidden: Arc<HashSet<UnitKey>>, pub layers: Vec<bool>, pub models: Vec<(bool, Vec<bool>)> }`;
  - `fn view_state(&self) -> ViewState`;
  - `fn restore_view_state(&mut self, s: ViewState)`. A state recorded before an Add has fewer layers and models; it restores those it has and leaves the newer ones as they are.
- Rules:
  - A change records only when the state actually differs, so there are no no-op entries.
  - `set_model` clears the history; `add_model` keeps it.
  - Selection and camera are not in the history.
- Commands: `edit.undo` "Ongedaan maken" / "Undo" with `Shortcut::ctrl('z')`, and `edit.redo` "Opnieuw" / "Redo" with `Shortcut::ctrl('y')`, enabled by `can_undo` and `can_redo`. They get no ribbon place, because the B2 set has no undo icons; they are reached by keyboard and from the command list.

- [ ] **Step 1: Write the failing tests**
  - `history.rs`: `undo_then_redo_round_trips`. Record 1, then `undo(2) == Some(1)` and `redo(1) == Some(2)`.
  - `history.rs`: `record_clears_redo`.
  - `app.rs`: `undo_restores_hidden_and_layers`. Hide 10 and switch layer 0 off. The first undo turns the layer back on, the second shows 10 again, and a redo hides it again.
  - `app.rs`: `no_entry_for_noop`. `ShowAllElements` with nothing hidden leaves `can_undo()` false.
  - `app.rs`: `undo_survives_add_model`. Hide in model a, then `add_model(b)`. An undo shows the element again.
- [ ] **Step 2: Run them to see them fail** → FAIL.
- [ ] **Step 3: Implement** `History`, `ViewState`, the recording and the commands.
- [ ] **Step 4: Run all tests** → PASS.
- [ ] **Step 5: Commit**: "Undo and redo visibility changes".

### Task 8: Box selection with Ctrl + drag

**Files:**
- Create: `src/viewport/box_select.rs`
- Modify: `src/viewport/mod.rs`: a Ctrl + left drag starts a box instead of an orbit, and publishes `Navigate::Box`.
- Modify: `src/app.rs`: `box_select: Option<(Vec2, Vec2)>` while dragging, the overlay in the viewport stack next to the measure overlay, and on release the units become selection items.

**Interfaces:**
- Consumes: `ExplodeIndex::unit_bounds`, `owner` (Task 4); the hidden units (Task 6).
- Produces:
  - `Navigate::Box { x0: f32, y0: f32, x1: f32, y1: f32, width: f32, height: f32, done: bool }`;
  - `pub fn units_in_box(index: &ExplodeIndex, offsets: &[[f32; 4]], view_proj: Mat4, size: (f32, f32), rect: (Vec2, Vec2), crossing: bool, hidden: &dyn Fn(usize) -> bool) -> Vec<usize>`. Here `view_proj` already includes the model matrix.
- Rules:
  - Dragging to the right (`x1 > x0`) is a **window**: units whose projected bounds, with their offset, lie wholly inside.
  - Dragging to the left is a **crossing**: units whose projected bounds overlap the box.
  - Units wholly behind the camera, and hidden units, are never taken.
  - A box always adds to the selection.
- Overlay: a 1 px outline in the selection amber, solid for a window and dashed for a crossing.

- [ ] **Step 1: Write the failing tests** in `box_select.rs`, with an orthographic `view_proj` and three units side by side:
  - `window_takes_units_fully_inside`;
  - `crossing_takes_units_touching`;
  - `hidden_units_are_not_boxed`.
- [ ] **Step 2: Run them to see them fail** → FAIL.
- [ ] **Step 3: Implement** `units_in_box`, the drag mode, the overlay and the app handling.
- [ ] **Step 4: Run all tests** → PASS.
- [ ] **Step 5: Commit**: "Select with a box: Ctrl and drag".

### Task 9: Select by layer and by property value

**Files:**
- Modify: `src/model.rs`
- Modify: `src/app.rs`:
  - `Message::SelectLayerOfSelection` and `Message::SelectWhere(String, String)`;
  - in the properties pane, a small button per property row with icon `view.fit-selection` and the tooltip "Alles met deze waarde selecteren" / "Select all with this value".
- Modify: `src/shell.rs`: the command `selection.layer`, "Laag selecteren" / "Select layer", enabled when the selection is not empty, placed in the "Selection" group.

**Interfaces:**
- Produces: `pub fn elements_with(&self, key: &str, value: &str) -> Vec<u64>` on `Model`. The key and value match exactly, ignoring case. The key `category` falls back to the element's first layer name, as `find_elements` does (`src/remote.rs:700-702`).
- **Select layer** selects every element and element-less layer in the layers that the current selection touches.

- [ ] **Step 1: Write the failing tests**
  - `model.rs`: `elements_with_matches_exactly_ignoring_case`. `"walls"` matches `"Walls"`, and `"Wall"` does not.
  - `app.rs`: `select_where_selects_all_matches`.
  - `app.rs`: `select_layer_of_selection`.
- [ ] **Step 2: Run them to see them fail** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all tests** → PASS.
- [ ] **Step 5: Commit**: "Select by layer and by property value".

### Task 10: Remote control and MCP

**Files:**
- Modify: `src/remote.rs`: dispatch at 266-345, `remote_select`, `state_json` at 521.
- Modify: `src/bin/open-3d-viewer-mcp.rs`: the tool list at 68-184.

**Interfaces:**
- Consumes: Tasks 4 to 7.
- New remote commands, with MCP tools of the same name:
  - `hide_elements {ids?}`: no `ids` means the selection;
  - `isolate_elements {ids?}`;
  - `show_all_elements`;
  - `undo` and `redo`.
- `select` gets `add: true`, which adds to the selection instead of replacing it.
- `get_state` gets `selection_count` and `hidden_count`.
- Each MCP tool gets a one-sentence description in the style of the existing ones.

- [ ] **Step 1: Write the failing tests** in the `remote.rs` test module (pattern `run(&mut app, "...", json!(...))` at 1426):
  - `remote_hide_and_show_all`;
  - `remote_select_add`;
  - `remote_undo_restores_hidden`.
- [ ] **Step 2: Run them to see them fail** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all tests** → PASS.
- [ ] **Step 5: Commit**: "Hide, isolate and undo through the MCP server".

### Task 11: Phase 1 exit: measurements and docs

**Files:**
- Modify: `src/bench.rs`
- Modify: `docs/performance/baseline.md`
- Modify: `README.md`, `README.nl.md`

**Interfaces:**
- Consumes: `unit_state`, `UNIT_SELECTED`, `UNIT_HIDDEN` (Task 3); `App` with `HideSelection` (Task 6).
- With `--throughput`, the bench also runs:
  - `orbit, all selected`: every unit `UNIT_SELECTED`;
  - `orbit, half hidden`: every second unit `UNIT_HIDDEN`.
- It also prints `hide {n} elements: {ms:.2} ms in update: {PASS|FAIL}`. This times `App::update(Message::HideSelection)` with everything selected, up to a new `units` vector, against the 8.33 ms budget of one frame.

- [ ] **Step 1: Implement** the extra scenarios and the timing line.
- [ ] **Step 2: Measure**: `cargo run --release -- --bench synthetic:100000 --throughput --frames 300`. Every line PASSES on the reference PC; on integrated graphics nothing is slower than the Phase 0 baseline. Add the rows to `docs/performance/baseline.md`.
- [ ] **Step 3: Update both READMEs:**
  - *Viewing and navigating*: a bullet on selecting (Ctrl-click, Ctrl and drag, by layer, by value), on hiding, isolating and showing all (`H`, `I`, `Ctrl+Shift+H`), and on undo (`Ctrl+Z`, `Ctrl+Y`).
  - *MCP*: the new commands.
  - *Current limitations*: remove "Elements cannot be hidden one by one"; change "No undo yet" to "**Undo covers visibility only.** Measurements and the section are cleared by hand."
  - *Testing and reliability*: the test counts, recounted from the repository.
- [ ] **Step 4: Verify**:
  - `cargo test --release --locked`;
  - `cargo test --release --locked -p openaec-ui --lib`;
  - the wasm check;
  - `python scripts/build_readme_images.py --check`.
  All must pass.
- [ ] **Step 5: Commit**: "Phase 1: measured, documented".
