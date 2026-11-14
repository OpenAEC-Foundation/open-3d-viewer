# Modeller: Select tool and shortcuts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** in the Modeller, selecting is a tool of its own (Select), a drag with it draws a selection box, and the keys of the reference modeller start the tools.

**Architecture:** the keys live in a Modeller module (`src/app/modeller/keys.rs`): a table and a pure two-letter sequence with its 750 ms window; the shell sends plain letter keys there while the Modeller is shown, before its command shortcuts. The Select tool is the state "no tool on"; a command and Esc lead there. The viewport gets one switch that turns a plain left drag into a box (the Modeller) instead of an orbit (the Viewer), and `Navigate::Box` says whether the box adds.

**Tech Stack:** Rust, iced 0.14 (shell, canvas, shader viewport), `iced_test` for interface tests; cargo with `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc` and the shared `target/`.

**Spec:** `docs/superpowers/plans/2026-11-14-modeller-roadmap.md`, sub-project 1. Reference: Open BIM Studio `main`, `ui/editor/bim_editor.dl` (keys 1345-2400, two-letter commands 964-1205, Esc 1697-1734, box 473-525) and `ui/shell/main.dl` (selection only with Select, 1820-1832).

## Global Constraints

- Modeller only: nothing changes while the Viewer or the Collaborator is shown; the Viewer's left drag still orbits and its Ctrl-drag box still adds.
- Undo stays Ctrl+Z, redo Ctrl+Y.
- A key is a Modeller key only without Ctrl or Alt and while no text field has the focus (`openaec_ui::keyboard::presses` sees only keys no widget took).
- Two letters within 750 ms are one command.
- Every interface text in every language of `locales/` (the test `i18n::tests::every_interface_text_is_translated`).
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; no other product names.

## Review Focus

1. A letter typed in a property field or the level form while the Modeller is shown must type, not start a tool. Task 2 pins it with `letters_typed_in_a_field_are_text`.
2. A first letter that is also a command of its own (C section, D door) must still run alone after 750 ms with no second key. Task 1 pins it with `expire`.
3. Esc while a two-letter command is pending drops the pending letter and does nothing else. Task 2.
4. A box drag in the Modeller that ends within 4 px of where it began is a click, which selects (or, with a tool on, belongs to the tool). Task 4.
5. PgUp on the highest level, PgDn on the lowest: nothing changes. Task 5.

---

### Task 1: The key table and the two-letter sequence

**Files:**
- Create: `src/app/modeller/keys.rs`
- Modify: `src/app/modeller.rs` (add `pub mod keys;` beside `pub mod browser;`)

**Interfaces:**
- Produces:
  - `pub enum ModelKey { Select, Wall, Slab, Column, Beam, System, Object, Door, Window, Grid, Section, Dimension, Grab, Move, Copy, Rotate, Mirror, Delete, LevelUp, LevelDown }`
  - `pub const SINGLE: &[(char, ModelKey)]` and `pub const PAIRS: &[(&str, ModelKey)]`
  - `pub struct KeySequence { pending: Option<(char, std::time::Instant)> }` with
    `pub fn press(&mut self, c: char, now: Instant) -> Vec<ModelKey>`,
    `pub fn expire(&mut self, now: Instant) -> Option<ModelKey>`,
    `pub fn pending(&self) -> Option<char>`, `pub fn clear(&mut self)`
  - `pub const WINDOW: Duration = Duration::from_millis(750)`

The table, from the reference modeller (lower-case letters):

| Single | Command | Pair | Command |
| --- | --- | --- | --- |
| s | Select | mv | Move |
| w | Wall | co | Copy |
| g | Grab (edit mode's G when editing, else Move) | ro | Rotate |
| i | Grid | mm | Mirror |
| d | Door | cl | Column |
| o | Window | bm | Beam |
| c | Section | di | Dimension |
| | | sb | Slab |
| | | cw | System |
| | | ob | Object |

PageUp and PageDown are not letters: the shell maps them to `LevelUp` and `LevelDown` directly (Task 5).

Rule of `press`: a letter that starts a pair waits (returns nothing) and becomes pending; if a letter is pending and the two make a pair within `WINDOW`, the pair's command is returned and nothing is pending; otherwise the pending letter's own command (if any) is returned first, then the new letter is handled as if nothing were pending. `expire` returns the pending letter's own command once `WINDOW` has passed, and clears it.

- [ ] **Step 1: Write the failing tests** in `keys.rs`:

```rust
#[test]
fn single_letters_run_at_once_unless_they_start_a_pair() {
    let t = Instant::now();
    let mut k = KeySequence::default();
    assert_eq!(k.press('w', t), vec![ModelKey::Wall]);
    assert_eq!(k.press('i', t), vec![ModelKey::Grid]);
    // c starts co and cl: it waits
    assert!(k.press('c', t).is_empty());
    assert_eq!(k.pending(), Some('c'));
    assert_eq!(k.expire(t + Duration::from_millis(500)), None);
    assert_eq!(k.expire(t + Duration::from_millis(760)), Some(ModelKey::Section));
    assert_eq!(k.pending(), None);
}

#[test]
fn two_letters_within_the_window_are_one_command() {
    let t = Instant::now();
    let mut k = KeySequence::default();
    assert!(k.press('m', t).is_empty());
    assert_eq!(k.press('v', t + Duration::from_millis(300)), vec![ModelKey::Move]);
    assert!(k.press('c', t).is_empty());
    assert_eq!(k.press('l', t + Duration::from_millis(100)), vec![ModelKey::Column]);
    // too late: the first runs alone, the second as itself
    assert!(k.press('d', t).is_empty());
    assert_eq!(k.press('w', t + Duration::from_millis(800)), vec![ModelKey::Door, ModelKey::Wall]);
    // a pending letter with no command of its own and no pair: nothing
    assert!(k.press('m', t).is_empty());
    assert_eq!(k.press('x', t + Duration::from_millis(100)), vec![]);
    assert_eq!(k.pending(), None);
}

#[test]
fn every_pair_and_letter_is_in_the_table_once() {
    let mut seen = std::collections::HashSet::new();
    for (c, _) in SINGLE { assert!(seen.insert(c.to_string())); }
    for (p, _) in PAIRS { assert!(seen.insert(p.to_string()) && p.len() == 2); }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib modeller::keys`
Expected: FAIL, `ModelKey` and `KeySequence` not found.

- [ ] **Step 3: Implement `keys.rs`** with the table above and the rule of `press` and `expire`; letters are compared lower-case.

- [ ] **Step 4: Run them to see them pass**

Run: `cargo test --lib modeller::keys`
Expected: 3 passed.

- [ ] **Step 5: Commit** `src/app/modeller/keys.rs` and `src/app/modeller.rs`.

### Task 2: The shell sends the Modeller's keys

**Files:**
- Modify: `src/shell.rs` (`Msg::Key` handling near the `EditKey` block, `subscription` near line 1691, `Studio` fields)
- Modify: `src/app.rs` (`Message::ModelKey(ModelKey)` and its arm)
- Create: `src/app/modeller/key_actions.rs` (`impl App { pub(in crate::app) fn model_key(&mut self, key: ModelKey) }`)

**Interfaces:**
- Consumes: `KeySequence`, `ModelKey`, `WINDOW` from Task 1.
- Produces: `Message::ModelKey(ModelKey)`; `App::model_key(&mut self, key: ModelKey)`; a field `keys: KeySequence` on `Studio`; `Msg::KeyWindow` sent every 100 ms by a subscription while `keys.pending()` is `Some`.

`model_key` does what the ribbon's command does: Wall, Slab, Column, Beam, System, Object, Door, Window → `set_model_tool(Some((tool, tool.label(l))))`; Grid → `toggle_grid_tool`; Section → the section tool of the plan shown; Dimension → the dimension tool of the view shown; Move, Copy, Rotate, Mirror → `begin_transform`; Grab → `edit_key('g')` in edit mode, else `begin_transform(Move)`; Delete → `DeletePicked`; Select → `select_tool` (Task 3).

- [ ] **Step 1: Write the failing tests** in `key_actions.rs`, driving a `Studio` in the Modeller (as `shell::tests` do with `studio.set_product(Product::Modeller)`):

```rust
#[test]
fn letters_start_the_modellers_tools() {
    let mut studio = modelling_studio();
    press(&mut studio, "w");
    assert_eq!(studio.app.modeller.tool.map(|t| t.0), Some(ModelTool::Wall));
    press(&mut studio, "c");
    press(&mut studio, "l");
    assert_eq!(studio.app.modeller.tool.map(|t| t.0), Some(ModelTool::Column));
}

#[test]
fn the_viewer_keeps_its_keys_and_a_text_field_its_letters() {
    let mut studio = modelling_studio();
    studio.set_product(Product::Viewer);
    press(&mut studio, "w");
    assert!(studio.app.modeller.tool.is_none(), "w walks in the Viewer");
    studio.set_product(Product::Modeller);
    // Ctrl or Alt with a letter is not a Modeller key
    press_with(&mut studio, "w", Modifiers::CTRL);
    assert!(studio.app.modeller.tool.is_none());
}

#[test]
fn letters_typed_in_a_field_are_text() {
    // the level form's name field has the focus: the keys go to it, `presses()` does not see them
    let mut studio = modelling_studio();
    let ui = iced_test::simulator(studio.view());
    ui.click("Level 0").ok(); // the Edit button of the level, then its name field
    ui.typewrite("west");
    for message in ui.into_messages() { let _ = studio.update(message); }
    assert!(studio.app.modeller.tool.is_none() && studio.keys.pending().is_none());
}

#[test]
fn esc_drops_a_pending_letter_only() {
    let mut studio = modelling_studio();
    press(&mut studio, "m");
    assert_eq!(studio.keys.pending(), Some('m'));
    escape(&mut studio);
    assert_eq!(studio.keys.pending(), None);
}
```

`modelling_studio`, `press`, `press_with`, `escape` are test helpers in the same module (a `Studio` of the Modeller with a new model; a `Msg::Key(KeyPress { .. })`).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib modeller::key_actions`
Expected: FAIL to compile, `Message::ModelKey` missing.

- [ ] **Step 3: Implement.** In `Msg::Key`, after the dialog and edit-mode checks and before `cm::find`: when the product is the Modeller, no dialog, no backstage, no Ctrl or Alt, and the key is one character → `self.keys.press(c, Instant::now())` and send each `Message::ModelKey`; PageUp / PageDown → `ModelKey::LevelUp` / `LevelDown`. Escape with a pending letter clears it and stops there. `Msg::KeyWindow` calls `expire`. The Viewer's walking keys stay under `Product::Viewer`.

- [ ] **Step 4: Run them to see them pass**, then the shell's own tests.

Run: `cargo test --lib modeller::key_actions shell::`
Expected: all pass.

- [ ] **Step 5: Commit** the three files.

### Task 3: The Select tool

**Files:**
- Create: `src/app/modeller/select.rs`
- Modify: `src/shell.rs` (command `model.select`, the Model tab's first group "Selecteren" / "Select"), `src/app.rs` (`Message::SelectTool` and the status line where `grid_hint` is asked), `locales/*.json`

**Interfaces:**
- Consumes: `model_key` (Task 2) calls `select_tool` for `ModelKey::Select`.
- Produces: `App::select_tool(&mut self)`, `App::selecting(&self) -> bool`, `App::modeller_escape(&mut self) -> bool`.

`selecting` is true when no modelling tool, no Grid tool, no transform, no edit mode, no measure tool and no dimension, text or section tool is on. `select_tool` switches all of them off and keeps the selection. `modeller_escape` is the Modeller's Esc, in order: a step under way (points clicked, a grab, a transform's clicks) is dropped; else a tool is switched off (back to Select); else the selection is cleared. It returns false when it did nothing, so the Viewer's Esc runs.

- [ ] **Step 1: Write the failing tests** in `select.rs`:

```rust
#[test]
fn select_is_on_when_no_tool_is_and_the_command_switches_tools_off() {
    let mut app = modelling();
    assert!(app.selecting());
    app.set_model_tool(Some((ModelTool::Wall, "Wall")));
    assert!(!app.selecting());
    let _ = app.update(Message::SelectTool);
    assert!(app.selecting() && app.modeller.tool.is_none());
}

#[test]
fn esc_drops_the_step_then_the_tool_then_the_selection() {
    let mut app = modelling();
    app.set_model_tool(Some((ModelTool::Wall, "Wall")));
    click(&mut app, 0.0, 0.0);
    let _ = app.update(Message::Escape);
    assert!(app.modeller.points.is_empty() && app.modeller.tool.is_some(), "the point goes first");
    let _ = app.update(Message::Escape);
    assert!(app.selecting(), "then the tool");
    let wall = app.add_element("Wall", None, wall_shape()).unwrap();
    app.select_modelled(wall);
    let _ = app.update(Message::Escape);
    assert!(app.selected_modelled().is_empty(), "then the selection");
}
```

A third test, `tools_end_as_the_reference_modeller_ends_them`: a wall is moved with the Move command and two clicks, after which `selecting()` holds; three clicks with the Wall tool make two walls and the Wall tool is still on; a click with the Door tool on a wall places a door and the Door tool is still on.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib modeller::select`
Expected: FAIL, `selecting` and `Message::SelectTool` missing.

- [ ] **Step 3: Implement** `select.rs`, the `model.select` command (icon `ui.select` if the icon set has it, else `object.edit`, `.mode(app.selecting())`), the ribbon group before Planar, and `Message::Escape` calling `modeller_escape` first while the Modeller is shown. The status line shows "Selecteren (S): klik of sleep een kader" / "Select (S): click or drag a box" while `selecting()` in the Modeller.

- [ ] **Step 4: Run them to see them pass**, then `modeller::` and `app::tests::`.

- [ ] **Step 5: Commit.**

### Task 4: A drag of the Select tool draws a box

**Files:**
- Modify: `src/viewport/mod.rs` (`Viewport3D` field, button press, `Navigate::Box`), `src/app.rs` (the `Viewport3D { .. }` literal near line 3606, the `Navigate::Box` arm near line 1881, `select_box`), `src/viewport/box_select.rs` (module doc)

**Interfaces:**
- Produces: `Viewport3D::drag_selects: bool`; `Navigate::Box { .., add: bool }`; `App::select_box(&mut self, rect, width, height, add: bool)`.

With `drag_selects`, a plain left drag is `Drag::Box` (Shift or Ctrl with it: adds); orbit stays on Shift + middle drag, pan on the middle and right buttons. Without it (the Viewer) nothing changes: Ctrl + left drag boxes and adds. A box that does not add replaces the selection. `drag_selects` is `in_modeller && selecting()`; with a tool on, a left drag orbits as before.

- [ ] **Step 1: Write the failing tests** in `src/app/modeller/select.rs`:

```rust
#[test]
fn a_box_replaces_the_selection_and_shift_adds() {
    let mut app = modelling_with_two_walls(); // walls 0..4 on y = 0 and on y = 6
    let (a, b) = (screen(&app, -1.0, -1.0), screen(&app, 5.0, 1.0));
    boxed(&mut app, a, b, false);
    assert_eq!(app.selected_modelled().len(), 1);
    let (c, d) = (screen(&app, -1.0, 5.0), screen(&app, 5.0, 7.0));
    boxed(&mut app, c, d, false);
    assert_eq!(app.selected_modelled().len(), 1, "replaced");
    boxed(&mut app, a, b, true);
    assert_eq!(app.selected_modelled().len(), 2, "Shift adds");
}
```

and in `src/app.rs`'s tests the existing `ctrl_drag_box_adds_what_it_holds` gets `add: true` and still passes.

- [ ] **Step 2: Run to see it fail** (`Navigate::Box` has no `add`).

- [ ] **Step 3: Implement** the field, the press rule, `add` on both `Navigate::Box` publishes, and `select_box` clearing first when `!add`.

- [ ] **Step 4: Run** `cargo test --lib modeller::select app::tests::` — all pass.

- [ ] **Step 5: Commit.**

### Task 5: The level worked on: PgUp and PgDn

**Files:**
- Modify: `src/app/modeller/key_actions.rs`, `src/app/modeller.rs` (`Modeller::level: Option<String>`, `work_plane`)

**Interfaces:**
- Consumes: `ModelKey::LevelUp`, `ModelKey::LevelDown`.
- Produces: `App::step_level(&mut self, up: bool)`.

`work_plane` takes, in order: the level of the floor plan shown, else `modeller.level`, else the level nearest zero (as now). `step_level` moves `modeller.level` to the next level up or down; when a floor plan is shown it opens that level's floor plan, making one when it has none. The status line names the level ("Level 1 (PgUp/PgDn)").

- [ ] **Step 1: Write the failing test:**

```rust
#[test]
fn page_up_and_down_step_through_the_levels() {
    let mut app = modelling();
    let _ = app.document.add_level("Level 1", 3.0);
    app.step_level(true);
    assert_eq!(app.work_plane().0, 3.0);
    app.step_level(true);
    assert_eq!(app.work_plane().0, 3.0, "no level above the highest");
    app.step_level(false);
    app.step_level(false);
    assert_eq!(app.work_plane().0, 0.0, "none below the lowest");
}
```

- [ ] **Step 2: Run to see it fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --lib modeller::` — all pass.
- [ ] **Step 5: Commit.**

### Task 6: Keys shown, texts translated, the app looked at

**Files:**
- Modify: `src/shell.rs` (the shortcuts page lists `keys::SINGLE` and `keys::PAIRS` under "Modeller" when the Modeller is shown), `locales/*.json`

- [ ] **Step 1:** a test in `key_actions.rs` that the shortcuts page of the Modeller shows "MV" and "Move".
- [ ] **Step 2:** run it to fail; implement; run the whole suite: `cargo test --lib` and `cargo test -p bim-core`, all pass, including `every_interface_text_is_translated`.
- [ ] **Step 3:** build the app, open the demo project in the Modeller, take a screenshot with a box being drawn and the status line showing Select; show it to the user.
- [ ] **Step 4: Commit**, push the branch `claude/modeller-select-and-keys`, open a PR against `main`.
