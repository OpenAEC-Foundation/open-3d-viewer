# GIS: where the project lies and what lies around it

**Status:** design of 2026-11-17; the first slice is built with it.

## Intent

A building stands somewhere. Before a design means anything one wants its parcel, the buildings
next to it, the street, the ground level. GIS in Open BIM Suite brings that around the model: the
place of the project on the map, and open map data fetched around it, in the same 3D view and
with the same selection and properties as a model.

The functions and their order follow Open Infra Studio, the GIS workbench of the OpenAEC
Foundation for 2D CAD drawings, and the way civil design programs start a project. What a new
user needs first, in this order:

1. the place of the project: an address searched, coordinates typed, or a click on a map;
2. the coordinate system that fits the place;
3. a base map under the model;
4. the area to fetch data for;
5. base data from open services: buildings, parcels, roads;
6. the terrain: heights from a service or a file;
7. GIS files in and out;
8. the coordinates under the cursor.

## Goals of the first slice

- Find a place by its address, street, town or postcode, or type its coordinates: a point of the
  Dutch national grid (`155000 463000`) or a latitude and longitude (`52.155, 5.387`).
- Convert between the Dutch national grid (RD New, EPSG:28992) and WGS 84.
- Fetch the parcels of the cadastral map and the buildings of the register of buildings within
  100, 250 or 500 m of the place, and add them to the project as models: each parcel and each
  building an element that can be picked, with what the register says about it as its properties.
- Put them where they belong: at their true place and turned as the model is when a model says
  where it lies on the grid (the map conversion of an IFC file).
- Name GIS on the product switch among the products to come.

## Non-goals (for now)

- A product of its own on the switch. The first slice is a tab of the right pane in every
  product; GIS becomes a product when it has its own ribbon.
- Countries other than the Netherlands. The grid is RD New and the services are Dutch. Other
  coordinate systems (automatic UTM, any EPSG code) and worldwide data come with the steps below.
- A base map in the 3D view. It needs textured ground in the renderer.
- Heights. Parcels and buildings lie flat at the zero level of the project.
- The browser build. It shows the tab and says it does not fetch yet.
- Writing the place into a file. It is kept while the project is open.

## Anti-goals

- Nothing of the model goes to a service. A request names the place or the square looked at and
  the app with its version, nothing else. The tab says so where the button is.
- No request without a press on a button. Nothing is fetched on opening a model or while typing.
- An answer cannot send the app elsewhere: the next page of features is followed only when it is
  an address of the same service.
- No value is invented. A building has no height in the register asked, so it is drawn flat.

## Design

### The crate `bim-gis`

Headless and without a network, so it builds for the browser and its tests need no connection:
it says what to ask a service and reads the answer; the app sends the request.

- `rd`: RD New to WGS 84 and back by the published series around Amersfoort. Within the
  Netherlands both stay within half a metre of the official transformation (0.30 m at six
  addresses from Maastricht to Delfzijl, in the tests). That places a building on a map; it is
  not for setting out. Also: coordinates as typed, and `Frame`, how the grid lies in a project (the
  grid point at the project's zero and the angle of its x axis from east).
- `pdok`: the open services of PDOK, the geodata platform of the Dutch government, which need no
  key: the place search, the cadastral map and the register of buildings and addresses (both
  OGC API Features, GeoJSON in RD New, 1000 features a page). It builds the addresses, reads the
  answers, and turns the features of a layer into one GeoJSON collection in the coordinates of
  the project. A parcel becomes its boundary lines and a building its area, so the two do not
  cover each other.

### In the app

`src/app/gis.rs`: the tab **GIS** of the right pane, beside Properties and Layers.

- **Location.** A field and a Search button. Coordinates are taken at once; anything else is
  asked of the place search, on a worker thread. The best place is chosen and the five best are
  listed to pick another. Without a chosen place the tab shows where the model says it lies.
- **Surroundings.** Which layers, the radius, and Load surroundings. The fetch runs on a worker
  thread: up to five pages per layer, then the existing GeoJSON reader makes a model of each
  layer, so picking, properties, layers, hiding and sections work as for any model. The models
  are named after their layer and the place. The tab then says how many parcels and buildings
  came, and when the area held more than was asked for.

### Where the data lands

- A model with a map conversion to RD New: grid points are turned into the model's own
  coordinates (its eastings and northings off, turned back by its rotation), then moved as the
  scene draws that model (its anchor and its move).
- No such model: the place of the first fetch is the zero of the project, north up. Later
  fetches, around other places too, use the same zero while a fetched model is open. A model
  that only names the latitude and longitude of its site gives that place as the zero.

The GeoJSON reader moves a file to a local origin, its first position. The fetch keeps that
position and moves the model back by it, so every fetched model lies in one frame.

## Next steps

In the order of the list above:

1. A base map under the model: OpenStreetMap, the national topographic map and the aerial photo
   as tiles on the ground of the 3D view.
2. More coordinate systems: UTM chosen by the place, other national grids, and the place search
   worldwide.
3. More base data: roads and land use, the large-scale topography, and buildings with their
   heights as blocks.
4. Heights: the national height model as terrain under the model, and the level of the ground at
   the project.
5. The place written into the project (IFCX) and into an IFC export as its map conversion.
6. Coordinates under the cursor in the status bar, in the grid and as latitude and longitude.
7. A ribbon of its own: GIS as a product on the switch.

## Sources

- Open Infra Studio (OpenAEC Foundation, GPL-3.0-only) for which functions come first and how
  they are grouped. No code is taken from it: this suite is LGPL-3.0-or-later, so the functions
  are written anew here.
- Parcels: Kadaster, BRK Kadastrale kaart. Buildings: Kadaster, BAG. Both through PDOK, open
  data; the source is kept as a property of every element.
