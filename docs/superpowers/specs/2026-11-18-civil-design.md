# Civil: the ground, roads, pipes and water around a building

**Status:** design of 2026-11-18; the first slice (terrain) is built with it.

## Intent

Open BIM Suite is to cover the whole built environment, not buildings alone: the ground a
project stands on, the roads to it, the pipes under it, the water around it. Civil is the part
of the suite for that: site and civil design in the same window, on the same project, with the
same 3D view, drawings, sheets and exports as the rest.

This note says what civil and site design programs hold, what the OpenAEC Foundation already has
of it, what the suite can build on, and in which order it is built here.

## What civil and site design holds

Surveyed from an open civil design program and from Open Infra Studio (the foundation's GIS and
civil workbench for 2D CAD drawings). By domain:

- **Survey**: points from surveyors' files (a number, coordinates, a height and a code, in any
  column order), codes that make lines and groups, traverses and their adjustment, parcels from
  deed calls, staking reports.
- **Terrain**: a surface (TIN) from points, breaklines and a boundary; from a height grid, a
  point cloud or contours; contours, height bands, slopes; volumes against a level or another
  surface.
- **Grading**: pads and ramps, feature lines, slopes that run to the ground (daylight),
  retaining walls; the proposed surface as the ground with the graded parts applied in order;
  cut and fill, a level that balances them, a haul plan.
- **Alignments and profiles**: an axis from tangents, arcs and transition curves, with
  stationing; the ground along it; a design profile with vertical curves.
- **Corridors**: a cross-section template (lanes, shoulders, kerbs, verges, side slopes, pavement
  courses) carried along an alignment and profile, with superelevation; cross sections, volumes,
  mass haul.
- **Road and site tools**: intersections with kerb returns and sight triangles, parking lots,
  vehicle swept paths, fences.
- **Rail**: transition types, cant, gauges and rail sections, turnouts, clearance.
- **Pipes and stormwater**: gravity networks with rules for slope and cover, flows from
  catchments, pipe sizing, grade lines; detention ponds; overland flow over the terrain.
- **Water mains and rivers**: pressure networks with pumps, valves and tanks; water surface
  profiles of a river with bridges and culverts.
- **Ground**: borehole and cone penetration logs, soil profiles, slope stability, settlement.
- **Utilities and power**: cables and pipes underground with crossings and trenches; overhead
  lines with sag, tension and clearances.
- **Landscape**: planting, beds, paving, walls, steps, irrigation, schedules.
- **Massing and environment**: building masses and zoning envelopes, sun and shade, wind, solar
  yield.
- **Exchange**: LandXML, IFC 4.3 (alignments, roads, earthworks, terrain), DWG and DXF, GIS
  files, reports as PDF and CSV.

What carries all of it, in the order each needs the one before: the project with its undo and
file; units and stations; points; the terrain surface; the alignment; the profile; grading and
the proposed surface; the corridor; sections and volumes; the pipe network.

## What there is

- **Open Infra Studio** (own, GPL-3.0-only, Rust): survey points, a TIN without enforced
  breaklines, contours, grid volumes, alignments of lines and arcs, profiles with vertical
  curves, cross sections, corridors with daylight and superelevation, LandXML reading, IFC 4.3
  export with alignments, the exact transformation between the national grid and ETRS89, a 3D
  model of terrain, buildings, topography, cables and soil investigations. Its civil code is
  written against its CAD host.
- **This suite**: no civil design yet. What civil can build on: clicking snapped points in the
  3D view and plans (the Modeller), polylines with arcs (the slab sketch), elements stored in
  IFCX with undo, cutting any mesh along a line or at a height (`drawing`), the 2D drawing model
  with its pens and every export format (`drawing2d`), lines and labels over the 3D view, terrain
  from LandXML, height grids and point clouds, and the place of the project on the map
  (`bim-gis`).

## Goals

- Every domain above, in the order below, as far as an open tool can honestly carry it.
- One engine crate without a GUI (`bim-civil`), so the app, the MCP tools and other OpenAEC
  programs use the same numbers.
- Numbers that are checked: each function has tests with answers worked by hand or taken from a
  published example.
- What civil makes is part of the project: stored in IFCX, undone and redone, drawn on sheets,
  exported as IFC 4.3, LandXML, DWG and DXF.

## Anti-goals

- No design value is invented. Speeds, radii, rainfall, roughness, cover and slopes come from
  the user or from a named source; a default that is only an example says so.
- No result without its inputs. A project stores what was typed and drawn; surfaces, contours,
  profiles and volumes are worked out from that.
- No code is taken from other programs. Open Infra Studio is GPL-3.0-only and this suite is
  LGPL-3.0-or-later; what is built here is written anew.

## Design

### The crate `bim-civil`

Headless: no GUI, no files, no network. Lengths in metres, x east, y north, z up.

- `points`: surveyors' point files. Commas, semicolons, tabs or spaces; a decimal comma where
  commas do not part the columns; a name in front or not; a code behind or not; north before
  east recognised in the Dutch national grid, or said by the caller.
- `tin`: the terrain surface.
  - Built by a constrained Delaunay triangulation: breaklines become edges, a boundary keeps the
    surface within an outline whose corners take the height of the surface, a longest edge takes
    the long triangles off the outside. Points within a millimetre count once; a breakline that
    crosses one laid before is left out where it crosses, and counted.
  - Or built from a grid of heights, with gaps left open or filled from their edge.
  - Gives the height at a place, the range, the area, contours as chained lines (closed where
    they close), the volume against a level exactly, and the volume against another surface by
    sampling.

Next modules, each on the ones before: `alignment`, `profile`, `grading`, `corridor`,
`sections`, `pipes`.

### In the app

GIS is a product on the switch since 2026-11-18, with a ribbon for the surroundings and the terrain. Terrain lives in its tab of the right pane, under the location and the
surroundings it belongs with (`src/app/terrain.rs`):

- **Load terrain** fetches the ground heights of the national height model around the place,
  at half a metre for 100 m, coarser for more, about 250 000 points. Gaps under buildings and
  water are filled from their edge.
- **Surface from a points file** triangulates a surveyor's file.
- Either becomes a model in the project: the ground as one element with its source, area and
  heights, and each contour a line that can be picked and says its height. The contour interval
  is 0.25, 0.5 or 1 m.
- In the national grid a surface lies where the other map layers lie. The project's zero level
  is at the height a georeferenced model gives; without one, at the ground of the place.

- Parcels and buildings fetched after a terrain lie on it: a boundary takes the height of the
  ground every few metres, a building lies flat at the highest ground under its outline.
- A level typed under the terrain gives the volume of the surface above and below it.

- **Load map** lays a map under the project as a coloured surface: the background map, the aerial
  photograph or a web map service of the user's own (`src/app/map.rs`, `bim-gis::map`). It takes the
  triangles of the terrain when there is one.
- A model is turned about its zero point as it is moved, so it is laid on the map as it stands.

The product is GIC since 2026-11-18: GIS, infrastructure and civil in one. Its ribbon has a page
**Geotechnics** and the right pane a tab for it (`src/app/geo.rs`, `bim-civil::ground`,
`bim-gis::bro`): cone penetration tests from the register of the subsurface around the place, or
from GEF files, each a column in the colours of the cone resistance with the resistance as a line
beside it. Borings come the same two ways: a column per boring, a block per described layer in
the colour of its main soil. The first model of the ground gives every place the layers of the
nearest boring, from that boring's ground down, on a grid of cells; nothing is laid between
borings. Next there: layers correlated between borings by the user (a boundary picked in each
boring, a surface through them), and the model cut along a line as a soil profile.

The ribbon of GIC has a page per domain: **GIS**, **Infra** (roads, pipes, cables), **Civil**
(structures: bridges, viaducts, tunnels), **Rail** and **Geotechnics**. Infra, Civil and Rail are
there and still empty; alignments, corridors and tracks land on them as their slices are built.

### Tiles, for areas without an end

Asked for on 2026-11-18 and not built yet. A map, a terrain and the surroundings are now one
square around a place, fetched once. For a road, a railway or a dyke of many kilometres the
project is to be cut in tiles of the national grid: each tile fetched when the view comes near,
at a detail that fits its distance, and let go when it is far, so the size of the project is not
bounded by memory. The tile grid of the background map (`bim-gis::map::Tiles`) is the start of
it; the terrain, the surroundings and what is designed along an alignment are to follow the same
grid. Coordinates far from the zero point are already kept by a shift per model.

## Slices, in order

1. **Terrain** (this slice): points, surfaces, contours, volumes in the engine; terrain from the
   height model and from a points file in the app.
2. **Terrain in use**: map layers draped on the terrain; height bands and slopes; breaklines and
   a boundary drawn in the view; cut and fill against a level or a second surface as a report;
   LandXML points and surfaces in and out.
3. **Alignments**: an axis by its intersection points with radii and transition curves,
   stationing, station and offset under the cursor; LandXML alignments; IFC 4.3 alignments.
4. **Profiles**: the ground along an alignment, a design profile with vertical curves, and the
   profile as a drawing on a sheet.
5. **Grading**: pads and feature lines with slopes to the ground, the proposed surface, a level
   that balances cut and fill.
6. **Corridors**: templates along an alignment and profile, superelevation, cross sections,
   volumes and mass haul.
7. **Pipe networks**: structures and pipes with rules for slope and cover, sizing, flows, grade
   lines, and the network in the profile.
8. **Site tools**: intersections, parking, swept paths, fences.
9. **Water**: detention, overland flow, pressure networks, river profiles.
10. **Ground**: cone penetration logs as columns (built with the first slice of geotechnics);
    borehole logs, soil profiles and a model of the ground between them, slope stability,
    settlement.
11. **Rail**: transitions, cant, turnouts, clearance.
12. **Underground and overhead**: cables and pipes from the cable register, crossings and
    trenches; overhead lines.
13. **Landscape, massing and environment**: planting and paving; masses and zoning; sun and
    wind. Environment is named on the product switch for this.

## Sources

- The national height model: Actueel Hoogtebestand Nederland, ground level, through the open
  coverage service of PDOK; open data. The source is a property of every surface made from it.
- The order of the functions: an open civil design program and Open Infra Studio, both read for
  what they do. No code is taken from either.
