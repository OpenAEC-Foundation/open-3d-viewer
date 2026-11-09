# Drawing export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export the view or sheet shown in its tab as DWG, DXF, IFCCAD, PDF, PNG and SVG.

**Architecture:** A headless `Drawing2d` is built once from a view (its cut, dimensions, notes)
or a sheet (paper, frame, title, placed views as the sheet canvas places them). Writers turn it
into bytes: CAD through `opencaddocument` (DWG, DXF, IFCCAD), a small PDF writer of our own, SVG,
and PNG from that SVG through resvg. An Export button in the view and sheet bars saves the file.

**Tech Stack:** Rust, iced 0.14, `opencaddocument` b9e1762 (acadrust, ifccad), resvg 0.45, png 0.18.

**Spec:** `docs/superpowers/specs/2026-11-09-drawing-export-design.md`

## Global Constraints

- Toolchain `cargo +1.99.1-x86_64-pc-windows-msvc`; the lib must also check for `wasm32-unknown-unknown`.
- Drawing units are metres; CAD output is millimetres (`insertion_units` 4), 1:1 for a view, paper
  size for a sheet.
- Pens (name, paper line weight): `O3V-Cut` 0.50 mm, `O3V-Beyond` 0.18 mm, `O3V-Dimensions` 0.18 mm,
  `O3V-Text` 0.25 mm, `O3V-Frame` 0.35 mm.
- Text 2.5 mm high on paper; a dimension line 6 mm (paper) to the left of from → to, its text the
  length in whole millimetres. A view's scale: that of the first sheet it is placed on, else 1:100.
- Sheet geometry exactly as `src/app/paper.rs` draws it: `FRAME` 0.005, inner border
  `[0.02, 0.01]`–`[w - 0.01, h - 0.01]`, title `number  name` at the inner border's upper right,
  a view's middle = middle of its cut's extent, crop clips lines.
- No names of other drawing programs in code, comments or docs.

## Review Focus

- A view or sheet with nothing cut (empty cut, no extent): every writer gives a valid, empty-ish file.
- Non-ASCII text (`ë`, `é`, `ü`, `€`) in notes and sheet names: CAD keeps it, PDF shows it (WinAnsi), SVG escapes `<`, `&`.
- A cropped view: lines outside the crop are left out in every format.
- A sheet whose view's cut has not been computed yet (view never opened): export computes it first.
- A sheet at A0 exported as PNG stays within a sane size (cap the longer side at 10 000 px).

---

### Task 1: The drawing (`src/drawing2d.rs`)

**Files:** Create `src/drawing2d.rs`; modify `src/lib.rs` (add `pub mod drawing2d;`).

**Interfaces — Produces:**
- `pub enum Pen { Cut, Beyond, Dimension, Text, Frame }` with `fn name(self) -> &'static str`, `fn weight_mm(self) -> f64`, `const ALL: [Pen; 5]`.
- `pub enum Space { Model { scale: f64 }, Paper }`; `Drawing2d::paper_per_unit(&self) -> f64` (1/scale or 1).
- `pub struct Line2d { pub pen: Pen, pub a: [f64; 2], pub b: [f64; 2] }`
- `pub enum Align { Left, Center, Right }`; `pub struct Text2d { pub pen: Pen, pub at: [f64; 2], pub height: f64, pub text: String, pub align: Align }` (`at` = lower left / bottom middle / lower right).
- `pub struct Dim2d { pub from: [f64; 2], pub to: [f64; 2], pub offset: f64, pub text_height: f64 }`; `fn lines(&self) -> Vec<[[f64; 2]; 2]>` (extension lines, dimension line, slashes) and `fn label(&self) -> Text2d` (centred beside the line).
- `pub struct Drawing2d { pub name: String, pub space: Space, pub lines: Vec<Line2d>, pub texts: Vec<Text2d>, pub dimensions: Vec<Dim2d> }`; `fn extent(&self) -> Option<([f64; 2], [f64; 2])>` (lines, texts, dimension lines); `fn page(&self) -> ([f64; 2], [f64; 2])` (paper: 0..paper; model: extent + 10 mm paper margin).
- `pub fn view_drawing(doc: &Document, path: &str, cut: &Cut) -> Option<Drawing2d>`
- `pub fn sheet_drawing(doc: &Document, path: &str, cut: &dyn Fn(&str) -> Option<Arc<Cut>>) -> Option<Drawing2d>`; `Space::Paper` with `paper` from the sheet (store it in `Drawing2d::paper: Option<[f64; 2]>`).

- [ ] Tests: `a_floor_plan_gives_cut_and_beyond_lines_its_dimension_and_note` (8 × 5 m room cut: 4 Cut lines, a dimension of 8 m labelled `8000`, a note); `a_crop_leaves_out_what_is_outside_it`; `a_sheet_places_a_view_at_its_scale_around_its_middle` (view at [0.2, 0.15], 1:100: an 8 m line becomes 0.08 paper m centred on 0.2); `a_sheet_has_its_frame_and_title`; `an_empty_cut_gives_a_drawing_without_lines`.
- [ ] Implement; run `cargo +1.99.1-x86_64-pc-windows-msvc test --lib drawing2d`; commit.

### Task 2: CAD writers (`src/export/cad.rs`)

**Interfaces — Consumes** Task 1. **Produces:** `pub fn cad_document(d: &Drawing2d) -> opencaddocument::Document`; `pub fn dwg(d: &Drawing2d) -> Result<Vec<u8>, String>`, `dxf`, `ifccad` (via `opencaddocument::{Dwg, Dxf, Ifcx, Format}`).

- One layer per pen with `LineWeight::Value(weight in 1/100 mm)`; lines → `Line`; texts → `Text` (height, horizontal alignment); dimensions → `DimensionAligned` with `definition_point` on the dimension line and a dim style `O3V` (`dimtxt`, `dimasz` = text height, ticks). Coordinates × 1000 (mm); paper space drawings likewise (paper mm).
- [ ] Tests: `a_drawing_round_trips_through_dwg_and_dxf` (write, read back with `Dwg.read`/`Dxf.read`: same count of lines, the texts' values incl. `Kamer ë`, the layers with their weights); `the_ifccad_drawing_validates` (`Ifcx.write` ok, read back lines count); `units_are_millimetres`.
- [ ] Implement; test; commit.

### Task 3: SVG and PNG (`src/export/svg.rs`)

**Produces:** `pub fn svg(d: &Drawing2d) -> String` (viewBox in paper mm of `page()`, y flipped, stroke widths = pen weights, texts in the app font family, escaped); `pub fn png(d: &Drawing2d, dpi: f64) -> Result<Vec<u8>, String>` (usvg with the app's fonts loaded in its `fontdb`, resvg to a white `tiny_skia::Pixmap`, longer side ≤ 10 000 px, encoded with `png`).

- [ ] Tests: `svg_has_a_path_per_pen_and_escapes_text`; `png_is_the_page_at_200_dpi_and_not_blank` (A3 → 3307 × 2339, some dark pixels); `a0_png_is_capped`.
- [ ] Implement; test; commit.

### Task 4: PDF (`src/export/pdf.rs`)

**Produces:** `pub fn pdf(d: &Drawing2d) -> Vec<u8>`: PDF 1.4, one page of `page()` in points, lines per pen (`w`, `m`, `l`, `S`), texts with Helvetica `WinAnsiEncoding` (characters outside it become `?`), xref table with correct offsets.

- [ ] Tests: `pdf_has_one_page_of_the_paper` (A3: MediaBox `0 0 1190.55 841.89`); `pdf_draws_the_lines_and_the_text` (count of ` l` ops; `(Kamer \353)`); `pdf_xref_offsets_point_at_objects`.
- [ ] Implement; test; commit.

### Task 5: Export button and saving

**Files:** Create `src/export/drawing.rs` (`pub const FORMATS: [(&str, &str); 6]` name/extension: DWG, DXF, IFCCAD `ifcx`, PDF, PNG, SVG; `pub fn write(d: &Drawing2d, file_name: &str) -> Result<Vec<u8>, String>` by extension); modify `src/export/mod.rs` (modules), `src/app.rs` (messages `ExportDrawing`, `ExportPicked(Option<PathBuf>)`, `Exported(Result<(PathBuf, f32), String>)`; the dialog task like `save_dialog_task`, the browser download like `save_task`), `src/app/marks.rs` (`view_bar`: an "Exporteren"/"Export" button), `src/app/paper.rs` (sheet bar: the same button).

- The drawing comes from the shown tab: a view tab → `view_drawing` with `cuts[view]` (call `cut_view` first when missing); a sheet tab → `sheet_drawing`, computing missing cuts of its views first. The file name offered: the view's or sheet's name with `.dwg`.
- [ ] Tests (app): `export_of_a_floor_plan_writes_a_dwg_that_reads_back`; `export_of_a_sheet_computes_the_cuts_of_its_views`.
- [ ] Implement; run the whole lib suite and the wasm check; commit.

### Task 6: README, check, PR

- [ ] README.md / README.nl.md: the export formats; test counts.
- [ ] `cargo test --lib`, `--bin open-3d-viewer-mcp`, wasm check, `python3 scripts/build_readme_images.py --check`.
- [ ] Show in the dev viewer; push; PR stacked on `claude/views-sheets`, referencing #100.
