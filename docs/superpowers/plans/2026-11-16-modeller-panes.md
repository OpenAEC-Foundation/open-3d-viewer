# Modeller: panes as in the reference modeller Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the Modeller's left pane is a project browser of views and building components, and its right pane shows a model explorer above the properties, as in the reference modeller.

**Architecture:** both live in Modeller modules: `src/app/modeller/browser.rs` (exists) and a new `src/app/modeller/explorer.rs`. The explorer is a tree worked out by a pure function from the modelled project and the document (rows with a depth, a label, a count and what a click does), drawn by a view function; which branches are open is Modeller state. The shell puts the explorer above the properties in the right pane only when the Modeller is shown.

**Tech Stack:** Rust, iced 0.14, `iced_test`; cargo with `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc` and the shared `target/`.

**Spec:** `docs/superpowers/plans/2026-11-14-modeller-roadmap.md`, sub-project 2. Reference: Open BIM Studio `main`, `ui/panels/bim_ui.dl` (project browser, 2443-2690) and `ui/panels/bim_model_explorer.dl` (tree, 252-356; clicks in `ui/shell/main.dl` 1530-1538).

## Global Constraints

- Modeller only; the Viewer's and the Collaborator's panes stay as they are.
- A branch lists at most 400 rows, then a row "and N more".
- The Tree / Graph switch is left out (roadmap decision).
- Every interface text in every language of `locales/`.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; no other product names.
- One branch: `claude/modeller-select-and-keys` (the user's wish).

## Review Focus

1. An element without a level (placed before levels existed, or taken over from a file without one) must still be listed: under "Zonder niveau" / "No level". Task 2.
2. A storey with more than 400 elements of one kind lists 400 and says how many more. Task 2.
3. The selection made in the viewport is marked in the tree, and the branch of a selected element opens. Task 2.
4. Removing an element that is listed (Delete, undo) leaves no stale row. Task 2 (the tree is worked out from the model each time it is drawn).
5. The right pane in a narrow (compact) window, where one pane is shown at a time, still shows the properties. Task 3.

---

### Task 1: The project browser in the reference modeller's groups

**Files:** Modify `src/app/modeller/browser.rs`.

**Interfaces:** Produces nothing new for later tasks; levels and grid lines leave the browser for the explorer (Task 2).

The browser lists, under VIEWS: floor plans (highest level first), the other plan kinds, 3D views, elevations, sections, sheets; under BUILDING COMPONENTS: walls, floors (slabs), columns and beams, building systems, objects, connections, each with its count. Levels and grid lines are in the model explorer now.

- [ ] **Step 1:** change `the_modeller_has_a_project_tab_that_lists_the_project` so it expects "Elevations" and "Sections" as separate groups when the project has one of each (`document.add_section_of(SectionKind::Elevation, ..)`, `add_section(..)`), and no "Levels" or "Grid lines" header; and `a_click_opens_a_levels_plan_picks_a_grid_line_and_selects_a_kind` loses its level and grid parts (they move to Task 2's tests).
- [ ] **Step 2:** run `cargo test --lib modeller::browser` and see it fail.
- [ ] **Step 3:** implement: split sections by `SectionKind`, drop the level and grid groups from the view (keep `BrowserMessage::Level` and `Grid`, the explorer sends them).
- [ ] **Step 4:** run it to pass; commit.

### Task 2: The model explorer

**Files:** Create `src/app/modeller/explorer.rs`; modify `src/app/modeller.rs` (`pub mod explorer;`, `Modeller::explorer_open: HashSet<String>`), `src/app.rs` (`Message::Explorer(ExplorerMessage)`).

**Interfaces:**
- Produces:
  - `pub struct ExplorerRow { pub depth: u8, pub key: String, pub label: String, pub count: Option<usize>, pub open: Option<bool>, pub picked: bool, pub on_click: ExplorerMessage }`
  - `pub enum ExplorerMessage { Toggle(String), Select(Vec<ElementId>), Level(Arc<str>), Grid(usize), View(Arc<str>), Sheet(Arc<str>) }`
  - `App::explorer_rows(&self, l: Language) -> Vec<ExplorerRow>` and `App::model_explorer(&self, p: Palette, l: Language) -> Element<'_, Message>`

The tree (keys in brackets; a branch shows its children when its key is in `explorer_open`; "project" is open from the start):

- Project [project]
  - Storeys [storeys], each level from the lowest [level:<path>], its elevation beside it; a click on the level row opens its floor plan (as the browser's `Level`)
    - each kind of element on it with its count [level:<path>/<class>], the kinds named as in the browser
      - each element by its name; a click selects it
  - No level [nolevel], only when an element has none, kinds and elements under it as under a level
  - Grid lines [grids], each by name and length; a click picks it
  - Types [types], each type used with its count (a wall's build-up, a system's type, a member's profile, by the library's name in the interface language, else the profile's designation); a click selects its elements
  - Views and sheets [views], each by name; a click opens it

Selecting in the viewport marks the element's row and opens its level, kind and the storeys (so the row is seen).

- [ ] **Step 1: tests** in `explorer.rs`:
  - `the_tree_lists_storeys_kinds_and_elements_lowest_first`: the demo building with grid lines; open "storeys" and the level and its walls; rows hold "Level 0" before "Level 1", "Walls" with count 3, three wall names, "Grid lines" with count 5 (after opening "grids").
  - `an_element_without_a_level_is_listed_apart`: an element added with level `None` shows under "No level".
  - `a_branch_lists_400_rows_and_says_how_many_more`: 405 objects on one level; the kind's branch has 400 element rows and "and 5 more".
  - `a_click_selects_toggles_and_the_selection_opens_its_branch`: `Message::Explorer(ExplorerMessage::Select(vec![id]))` selects it; selecting a wall in the viewport (`select_modelled`) makes its row `picked` and its branches open; `Toggle("grids")` opens and closes.
- [ ] **Step 2:** run `cargo test --lib modeller::explorer`: fail.
- [ ] **Step 3:** implement `explorer_rows` (pure, from `modeller.model`, `document`, the selection), the messages, and `model_explorer` (rows indented by depth, a chevron for branches, the count right, the picked row in the accent).
- [ ] **Step 4:** run to pass; commit.

### Task 3: The explorer above the properties

**Files:** Modify `src/shell.rs` (the right pane, both the wide and the compact layout).

In the Modeller the right pane holds, under the title "Modelverkenner" / "Model explorer", the explorer in its own scroll area of 40 % of the pane's height, a rule, then the properties as now. Elsewhere the pane is as it was.

- [ ] **Step 1: test** in `shell::tests`, `the_modellers_right_pane_shows_the_explorer_above_the_properties`: a Studio in the Modeller with a new model finds "MODEL EXPLORER" (the section label) and "Properties"; in the Viewer it does not find "MODEL EXPLORER".
- [ ] **Step 2:** run: fail. **Step 3:** implement. **Step 4:** run the whole suite and the translation test: pass.
- [ ] **Step 5:** build the app, take a screenshot of the demo project in the Modeller with a wall selected, show it; commit and push the branch.
