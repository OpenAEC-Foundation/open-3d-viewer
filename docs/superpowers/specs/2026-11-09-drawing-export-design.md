# Drawing export: DWG, DXF, IFCCAD, PDF, PNG and SVG

Part 1 of three for 2D drawings (issue #100). Part 2 loads DWG, DXF, IFCCAD and PDF drawings as
underlays on a level. Part 3 shows views, the viewer's own and those read from Revit, in 3D on
their plane.

## Intent

The viewer draws floor plans, sections and elevations of a model and places them on sheets
(views and sheets, #106). Those drawings have to reach other people and programs: a CAD user
wants a DWG or DXF of a floor plan, a client wants a PDF of a sheet, a report wants a PNG. IFCCAD,
the open 2D drawing profile on IFCX, is the format the other OpenAEC tools share, so a drawing
goes out as IFCCAD as well.

## Goals

- Export the view or sheet shown in its tab as DWG, DXF, IFCCAD (`.ifcx`), PDF, PNG and SVG.
- One drawing, six writers: what is exported is built once from the view or sheet (its cut lines,
  the lines beyond, its dimensions and texts, the sheet's frame and title), and every writer
  writes that.
- Lines keep their meaning: cut lines bold, lines beyond thin, dimensions and texts on their own
  layers, so a CAD user can switch them per layer.
- A view comes out at true size (millimetres in CAD, at its sheet scale on paper); a sheet comes
  out at paper size.
- Works on the desktop and in the browser (a download there).

## Non-goals

- Paper-space layouts with viewports in DWG: a sheet is written as drawn on paper, in model
  space at 1:1 millimetres. Layouts can follow.
- Hatches of cut elements, line patterns, fonts other than a plain sans serif.
- Reading drawings (part 2) and views in 3D (part 3).

## How

- **The drawing** (`src/drawing2d.rs`, headless): layers with a line weight and a colour, and
  items on them: lines, texts (position, height, rotation) and dimensions (two points, the line's
  offset, the text). It is in metres, either model metres (a view) or paper metres (a sheet), and
  records which.
- **Building it**: from a view, its cut and the document's dimensions and notes of that view;
  from a sheet, its paper, frame and title, and each placed view moved, scaled and cropped as the
  sheet draws it.
- **CAD** (`src/export/cad.rs`): the drawing becomes an `opencaddocument::Document` (layers with
  line weights, lines, texts, aligned dimensions) in millimetres. `opencaddocument` (MPL-2.0)
  writes it as DWG and DXF through its codec, and as IFCCAD through `ifccad`.
- **PDF** (`src/export/pdf.rs`): a small vector PDF writer of its own: lines with their weights
  and Helvetica text (WinAnsi), one page at the sheet's paper or around the view at its scale.
- **SVG and PNG** (`src/export/svg.rs`): the drawing as SVG; for PNG that SVG is rendered with
  resvg (already built for iced) at 200 dpi and written with the `png` crate.
- **UI**: an Export button in the bar over a view's tab and a sheet's tab. It asks where to save,
  with one filter per format; the extension picks the writer. In the browser the file goes to the
  downloads.

## Builder's calls

- The CAD loader reads through `opencaddocument`'s `acadrust` (the same repository at a newer
  revision, with its parallel reading on the desktop) rather than its own older copy, so the
  viewer builds one DWG codec. Its tests read the same as before; a DWG that reads differently
  goes back to a separately pinned `acadrust` for the loader.
- Text height on a view: 2.5 mm on paper at the view's scale.
- Layer names: `O3V-Cut`, `O3V-Beyond`, `O3V-Dimensions`, `O3V-Text`, `O3V-Frame`.
