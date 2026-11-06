# sam-json v1

`sam-json` is the vendor-neutral exchange format of a **structural analysis model**: nodes,
members, surfaces, supports, releases, sections, materials, load cases, combinations, actions and
results. It is a file format (`.samjson`), the payload of the `sam_*` C ABI that FE-package
readers implement, and the format of the test fixtures.

- JSON Schema: [`../schema/sam-json-v1.schema.json`](../schema/sam-json-v1.schema.json), generated
  from the serde types in `src/exchange/wire.rs` (a test fails when the committed file is stale).
- Reader and writer: `bim_structural::exchange::{read, write}`.
- Conformance check: `cargo run -p bim-structural --bin sam-check -- model.samjson [blocks.samr...]`.
- Example: [`../tests/fixtures/portal_frame.samjson`](../tests/fixtures/portal_frame.samjson).

Unknown fields are rejected everywhere. A producer gives only what its source holds and never
invents data: where a source has only a type name for a support or release, it writes the name.

## Conventions

| Topic | Rule |
|---|---|
| Units | SI base units in f64: m, N, N·m, N/m, N/m² (Pa), rad, K. Translational springs N/m, rotational springs N·m/rad, line-support springs per metre (N/m², N·m/rad/m). |
| Global frame | Right-handed, Z up, gravity −Z. A source with gravity +Z is rotated 180° about X, `(x, y, z) → (x, −y, −z)`, never mirrored, and says so in `global_transform` and `gravity_was`. |
| Positions | World metres of the source. `origin` is an anchor near the model that viewers subtract for display. |
| Member frame | x from start to end (the tangent for curved members); z the given `z`, perpendicular to x; y = z × x. Profile u (width) maps to local y and profile v (height) to local z. |
| Internal forces | The positive cut face has outward normal +x. Forces on it act along +x, +y, +z (N > 0 is tension); moment vectors along +x, +y, +z. `σ = N/A + My·z/Iy − Mz·y/Iz`: +My puts tension on the +z fibre, +Mz on the −y fibre. |
| Reactions | The force the support exerts on the structure, in the support frame. |
| Stations | Absolute metres from the member's start node along the analytical member (rigid end offsets do not shift them). A jump is two stations with the same x, left value first. |
| Matrices | Column-major. 4×4 (`global_transform`, `shared_frame`): 16 numbers, translation at indices 12-14, last row `0 0 0 1`. 2D affine (`transform`, `profile_position`): `[xx, xy, yx, yy, tx, ty]`. Node frames: the frame's x, y and z axes as three vectors. |

## Document

```json
{
  "format": "sam-json", "version": 1, "units": "SI",
  "source": { … }, "origin": [x, y, z],
  "models": [ { … } ]
}
```

### `source`

| Field | Type | Default | Meaning |
|---|---|---|---|
| `application` | string | — | Producing program. |
| `file` | string | — | Source file name. |
| `global_transform` | 16 numbers | identity | Rigid transform applied to reach the Z-up frame. |
| `conventions.gravity_was` | `"-z"` \| `"+z"` | `"-z"` | Gravity direction of the source. |
| `conventions.mz_sign_was` | `"tension_minus_y"` \| `"tension_plus_y"` | canonical | The source's Mz sign before conversion. |
| `conventions.reactions_were` | `"on_structure"` \| `"on_support"` | canonical | The source's reaction sense before conversion. |
| `conventions.result_signs` | `"verified"` \| `"assumed_canonical"` | `"assumed_canonical"` | `verified` only when the producer has confirmed its sign mapping. |
| `conventions.local_z_rule` | string | — | How the source defined its default member frame (text, never interpreted). |
| `eccentricity_policy` | `"explicit"` \| `"cardinal_cancels_position"` \| `"additive"` | `"explicit"` | How the insertion offsets combine; with `explicit` nothing is inferred by the reader. |

### Models

| Field | Type | Default |
|---|---|---|
| `id` | integer | |
| `name` | string | |
| `dimension` | `"3d"` \| `{"2d": {"normal": [x, y, z]}}` (unit normal) | `"3d"` |
| `kind` | `"analytical"` \| `"physical_axes"` | `"analytical"` |
| `shared_frame` | 16 numbers (IFC SharedPlacement, world metres) | identity |
| `materials`, `sections`, `nodes`, `members`, `surfaces`, `line_supports`, `load_cases`, `combinations`, `actions`, `results`, `links`, `diagnostics` | arrays | `[]` |

## Ids

`sam_id(kind, number) = kind << 56 | number`, `number` the source number in the low 56 bits:

| Kind | Byte | Kind | Byte |
|---|---|---|---|
| node | 1 | load case | 7 |
| member | 2 | combination | 8 |
| surface | 3 | action | 9 |
| section | 4 | result set | 10 |
| material | 5 | opening | 11 |
| line support | 6 | | |

Nodes, members, surfaces and line supports **must** use the shifted form (node 1 is
`72057594037927937`). Sections, materials, load cases, combinations, actions and result sets may
use small plain numbers; the reader shifts them, and a shifted id must carry its own kind. Ids
are unique within a model and stable for the same file across opens. `no` keeps the source
numbering as text.

## Materials

`{"id", "name", "E", "G", "nu", "rho", "alpha_t"}`: moduli in Pa, density in kg/m³, thermal
expansion in 1/K. All values optional.

## Sections

```json
{ "id": 1, "name": "HEB 200", "material": 1,
  "shape": { "I": { "h": 0.2, "b": 0.2, "tw": 0.009, "tf": 0.015, "r": 0.018, "slope": 0 } },
  "props": { "A": 7.81e-3, "Iy": 5.696e-5, "Iz": 2.003e-5, "It": 5.93e-7 } }
```

Shapes, in metres, with the origin at the bounding-box centre of the profile (u right, v up):

| Shape | Parameters | Orientation and meaning |
|---|---|---|
| `I` | `h b tw tf r slope` | Symmetric I; `r` root fillet; `slope` inner flange slope (rad), `tf` measured at \|u\| = b/4. |
| `AsymI` | `h bt bb tw tft tfb r` | Monosymmetric I; `bt`/`tft` top flange (+v). |
| `U` | `h b tw tf r1 r2 slope` | Channel, web at −u, flanges towards +u; `r1` root, `r2` toe (inner edge) radius; `tf` measured at u = 0 (b/2 from the toe). |
| `L` | `h b t r1 r2` | Angle, heel at (−b/2, −h/2); `h` leg along +v, `b` leg along +u; `r1` root, `r2` toe radius. |
| `T` | `h b tw tf r` | Flange at +v. |
| `C` | `h b t lip r` | Cold-formed lipped channel, web at −u; `r` inner bend radius; `lip` 0 for a plain channel. |
| `Z` | `h b tw tf r` | Web on v; top flange towards −u, bottom towards +u; `b` per flange including the web. |
| `Rect` | `b h` | |
| `Circle` | `d` | |
| `Chs` | `d t` | Circular hollow section. |
| `Rhs` | `b h t r_out r_in` | Rectangular hollow section. |
| `polygon` | `outer: [[u, v], …]`, `holes: [[[u, v], …], …]` | Any orientation; the reader orients outer loops counter-clockwise and holes clockwise. |
| `composite` | `[{"shape": …, "transform": [xx, xy, yx, yy, tx, ty]}, …]` | Non-overlapping parts. |
| `"unknown"` | — | Name only. |

Radii and slopes default to 0. `props` (`A`, `centroid`, `shear_centre`, `Iy`, `Iz`, `Iyz`, `It`,
all optional) are the source's values: they win for labels, never for geometry. The reader
computes area, centroid and second moments exactly from the outline (lines and arcs), and the
shear centre from symmetry or a closed form (`L`, `T`, `U`, `C`, `AsymI`); for `polygon` and
`composite` it uses the imported `shear_centre` when given, else the centroid (flagged
approximate). `shear_centre` is needed for cardinal points 15-19 on such shapes.

## Nodes and supports

```json
{ "id": 72057594037927937, "no": "1", "p": [0, 0, 0], "frame": null,
  "support": { "dofs": ["fixed", "fixed", "fixed", "free", "free", "free"], "frame": "global" } }
```

- `frame`: optional skewed support frame (three unit vectors, orthonormal, right-handed).
- `origin`: `"analytical"` (default) or `{"derived_from_physical": {"element": id, "approximate": bool}}`.
- `support`: `dofs` (six DOFs `ux uy uz rx ry rz`), or `named` (a type from the table below), or
  both (the DOFs win; the name is kept). `frame` is `"global"` (default), `"node"` (needs the
  node's `frame`) or `{"member": id}` (a member at this node).

A DOF is `"fixed"` (restrained; at member ends: connected), `"free"` (released), `{"spring": k}`,
`{"tension_only": k}`, `{"compression_only": k}`, `{"failure": {"k": k, "limit": f}}`,
`{"slippage": {"k": k, "gap": g}}` or `{"diagram": [[d, f], …]}`. Negative stiffness is kept and
flagged.

DOF codes (UI and properties): `1` restrained/connected, `0` free/released, `s` spring,
`n` nonlinear.

| Support name (case-insensitive) | DOFs | Notes |
|---|---|---|
| `fixed`, `clamped` | `111111` | |
| `pinned`, `hinged`, `hinge` | `111000` | |
| `roller` | `001000` | free in both horizontal directions of the support frame |
| `roller_x`, `roller_y` | `011000`, `101000` | free along the named axis only |
| `sliding_x`, `sliding_y` | `011111`, `101111` | guided: rotations fixed |
| `free` | `000000` | |

A name not in the table is read as a custom support with every DOF free and an
`unknown-support-name` diagnostic; `sam-check` warns. An expanded name adds
`named-support-expanded`.

## Members

```json
{ "id": 144115188075855873, "no": "1", "kind": "beam",
  "nodes": [72057594037927937, 72057594037927938], "geometry": "line",
  "z": [1, 0, 0], "orientation": { "beta": { "angle": 0.0, "rule": "…" } },
  "section": [1, null], "section_rotation": 0.0,
  "insertion": { "cardinal_point": 8, "eccentricity": [0.0, 0.0],
                 "end_offsets": { "start": [0, 0, 0], "end": [0, 0, 0], "frame": "local" } },
  "releases": [null, { "dofs": ["fixed", "fixed", "fixed", "fixed", "free", "free"] }] }
```

| Field | Meaning |
|---|---|
| `kind` | `beam`, `truss`, `cable`, `tension_only`, `compression_only`, `rigid`, `spring`. |
| `nodes` | Start, intermediate (ordered along the member), end. |
| `geometry` | `"line"` (default), `{"arc": {"mid": [x, y, z]}}` (circular arc through `mid`), or `{"polyline": [[x, y, z], …]}` with the vertices strictly between the end nodes. |
| `z` | **Always given**: a unit vector perpendicular to the axis at the start, resolved with the source's own default rule and rotation angle. |
| `orientation` | Provenance only, never interpreted: `"default"`, `"ifc_axis"`, `{"beta": {"angle": rad, "rule": text}}`, `{"ref_node": id}`, `{"ref_vector": [x, y, z]}`. |
| `section` | `[start, end]` section ids (`end` for tapers, else `null`), or `null`. |
| `section_rotation` | Extra rotation of the profile about x, rad. |
| `insertion.cardinal_point` | IFC4 `IfcCardinalPointReference` 1-19, 0 = none (below). |
| `insertion.profile_position` | The profile definition's own Position, as read (IFC). |
| `insertion.profile_offsets` | IFC `IfcMaterialProfileWithOffsets` at start and end. |
| `insertion.eccentricity` | Member-wide (ey, ez) in local axes, applied after the cardinal point: the profile moves by +e. |
| `insertion.end_offsets` | Rigid offsets from node to member end, `frame` `"local"` or `"global"`. |
| `releases` | Start and end: `null` (rigid) or `dofs` (member local; `fixed` = connected) and/or `named` (table below). |
| `material`, `links` | Material id; physical element ids in the same source. |

The producer splits every source offset into exactly one of `cardinal_point`, `eccentricity` and
`end_offsets`. A section placed by a reference point that is not one of the 19 cardinal points
gets `cardinal_point: 0` and the full offset of the profile's bounding-box centre in
`eccentricity`.

Cardinal points, looking along the member from start to end (left is +u, top is +v):
1-9 points of the profile's bounding box (bottom row 1 left, 2 centre, 3 right; mid-depth 4-6;
top 7-9), 10 the centroid, 11-14 the centroid projected onto the bottom, left, right and top
edges, 15 the shear centre, 16-19 the shear centre projected likewise.

| Release name | DOFs | Notes |
|---|---|---|
| `rigid` | `111111` | no release |
| `hinge`, `moment_hinge` | `111100` | ry and rz released |
| `hinge_y`, `hinge_z` | `111101`, `111110` | one bending axis released |
| `torsion` | `111011` | rx released |
| `axial` | `011111` | ux released |

An unknown release name keeps the end connected, with `unknown-release-name`.

## Surfaces and line supports

Surfaces: `{"id", "no", "kind", "boundary": [node ids], "openings": [[node ids]], "thickness",
"offset_z", "x_dir", "material", "links"}`. `kind` is `plate`, `membrane`, `shell`, `rigid` or
`load_transfer`; `thickness` is `{"uniform": t}`, `{"linear": [[x, y, z, t] ×3]}` or `"unknown"`.

Line supports: `{"id", "no", "along", "dofs" | "named", "frame"}`; `along` is `{"member": id}`,
`{"surface_edge": {"surface", "from", "to"}}` or `{"polyline": [[x, y, z], …]}`; springs per
metre; `frame` `"global"` or `{"member": id}`.

## Loads

- Load cases: `{"id", "no", "name", "action_type": "permanent" | "variable" | "accidental" |
  {"user": text}, "action_source": "DEAD_LOAD_G" …, "self_weight": [fx, fy, fz] | null,
  "coefficient": 1.0}`.
- Combinations: `{"id", "no", "name", "purpose": "uls" | "sls" | "als" | {"user": text},
  "terms": [[{"case": id} | {"combination": id}, factor], …]}`. Shown, never evaluated.
- Actions: `{"id", "load": {"case": id} | {"combination": id}, "target", "frame", "kind",
  "destabilising"}`.
  - `target`: `{"node": id}`, `{"member": id}`, `{"member_span": {"member": id, "span": [a, b]}}`
    (absolute metres), `{"surface": id}`, `{"free": [[x, y, z], …]}`.
  - `frame`: `"global"`, `"local"` or `"global_projected"` (projected only with global
    directions).
  - `kind`: `{"nodal_force": {"f", "m"}}`, `{"member_point": {"x", "f", "m"}}` (absolute `x`),
    `{"member_distributed": {"dir", "moment", "dist"}}`, `{"surface": {"dir", "dist"}}`,
    `{"temperature": {"dt", "dt_y", "dt_z"}}`, `{"displacement": {"u": [6 × number | null]}}`.
  - Member distributions (intensity as given in its frame, x absolute metres): `{"uniform": q}`,
    `{"linear": {"q1", "q2"}}`, `{"trapezoidal": {"a", "b", "q1", "q2"}}`,
    `{"polygonal": [[x, q], …]}`, `{"parabolic": peak}`, `{"sine": peak}`.
  - Surface distributions: `{"uniform": p}`, `{"bilinear": [p1, p2, p3]}`,
    `{"sampled": [[u, v, p], …]}`.

## Results

```json
{ "id": 1, "no": "CO1", "name": "ULS 1", "for": { "combination": 1 },
  "theory": "first_order", "linear": true,
  "tables": ["member_forces", "reactions"],
  "inline": { "member_forces": "U0FNUgEAAQA…", "reactions": "…" } }
```

- `for`: `{"case": id}`, `{"combination": id}` or `{"envelope": text}`.
- `theory`: `first_order`, `second_order`, `third_order`, `full_nonlinear`.
- `tables`: the tables the set has: `member_forces`, `member_deformations`, `node_displacements`,
  `reactions`, `envelope`.
- `inline`: optional, standard base64 of result blocks by table name (small models). Otherwise
  the blocks come from `sam_result_block` or sidecar files `<name>.<set>.samr`.

Internal forces use the canonical sign convention above (or are converted, with `mz_sign_was`
saying from what); reactions are the force on the structure. The producer never computes results
the source does not hold.

### Result block

Little endian, no padding:

```
magic     "SAMR"            4 bytes
version   u16 = 1
kind      u16               1 member forces, 2 member deformations, 3 node displacements,
                            4 reactions, 5 envelope
set       u64               result set id
rows      u32               members or nodes
stations  u32               total stations (member kinds); 0 for node kinds
ids       u64[rows]
offsets   u32[rows + 1]     member kinds only; stations of row i are offsets[i]..offsets[i + 1]
x         f64[stations]     member kinds only
values    f64[6][n]         component-major; n = stations (member kinds) or rows (node kinds)
                            member forces: N Vy Vz Mt My Mz; deformations: ux uy uz rx ry rz (local)
                            node displacements: ux uy uz rx ry rz (global);
                            reactions: Fx Fy Fz Mx My Mz (support frame)
```

The envelope (kind 5) has the member layout with `values` as `f64[12][stations]` (max N … Mz,
then min N … Mz), followed by `u64[6][stations]` governing result set ids for the maxima and
`u64[6][stations]` for the minima. A block has no trailing bytes.

## Links and diagnostics

- `links`: `[{"analytical": id, "physical": id, "source": "ifc_rel_assigns_to_product" |
  "revit_association" | {"matched": {"distance": d}}}]`.
- `diagnostics`: `[{"object": id | null, "code": "…", "message": "…"}]` for everything the
  producer could not map.

## Conformance rules

`sam-check` and `exchange::read` apply these rules; each has its own code in the output
(`error[member-z] /models/0/members/1/z: …`).

| Code | Rule |
|---|---|
| `schema` | The text parses as the schema: types, required fields, no unknown fields. |
| `format` | `format` is `"sam-json"`, `version` 1, `units` `"SI"`. |
| `finite` | No NaN or infinity (in the document or in result blocks; numbers beyond f64 in the text). |
| `id-kind` | Nodes, members, surfaces and line supports use `sam_id` with their kind; other shifted ids carry their own kind. |
| `id-unique` | Model ids unique in the document; item ids unique in their model. |
| `node-ref`, `member-ref`, `surface-ref`, `section-ref`, `material-ref` | Every referenced item exists. |
| `load-ref` | Action loads and combination terms name existing cases or combinations. |
| `result-ref` | Result sets are for an existing case or combination. |
| `link-ref` | Links start at a node, member or surface of the model. |
| `member-nodes` | A member has at least two nodes, distinct ends, no repeated node and a non-zero length. |
| `member-z` | `z` has unit length and is perpendicular to the axis at the start, within 1e-9. |
| `cardinal-point` | Cardinal points are 0-19. |
| `dofs` | Supports and releases give `dofs` or `named`. Warnings: a name not in the tables, a negative stiffness. |
| `dof-frame` | A `"node"` support frame needs the node's `frame`; a `{"member": id}` frame names a member at that node; line supports have no node frame. |
| `node-frame` | Node frames are orthonormal and right-handed within 1e-9. |
| `rigid-transform` | `global_transform` and `shared_frame` are rotations plus translations (no scale, no mirror). |
| `dimension` | A plane model's normal has unit length. |
| `section-shape` | Section dimensions are positive and fit together (flanges, fillets and lips inside the profile). |
| `surface-boundary` | Surface boundaries have at least three distinct nodes and a positive thickness; line-support polylines at least two points. |
| `load-position` | Member load positions and spans lie on the member (0..L) and ascend. |
| `result-block` | Inline and sidecar blocks decode, are listed in `tables`, belong to their set and refer to existing members or nodes. |
| `stations` | Stations ascend within 0..L (1e-6 m), with jumps as exact pairs (never three equal stations). |

## Reader behaviour

`exchange::read` validates first and refuses a document with errors. It then shifts plain ids,
expands named supports and releases (adding diagnostics), classifies support presets, computes
section properties from the shapes, sorts actions by load case, decodes inline blocks and sets
`source.format` to `sam-json/1`. `exchange::write` always writes supports and releases with
their DOFs (and their name when they have one) and ids in shifted form; `read(write(d)) == d`.
