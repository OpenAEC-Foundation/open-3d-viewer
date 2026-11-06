# Frame budget baseline

The 120 fps criterion of the [BIM modeller design](../superpowers/specs/2026-11-06-bim-modeller-design.md) for camera-only navigation: at least 120 submitted frames per second and a p95 frame time of at most 8.33 ms, with the frames over 16.67 ms counted. It is hard on the reference PC with a discrete GPU. On integrated graphics the rule is no regression against this baseline.

## How to measure

```bash
cargo run --release -- --bench synthetic:100000 --throughput --frames 300
cargo run --release -- --bench test-models/test-kavel.skp --throughput --frames 300
cargo run --release -- --bench path/to/large.ifc --throughput --frames 300
```

`--throughput` renders offscreen at 1920x1080 with 4x MSAA in solid mode. It keeps two frames in flight, as a swap chain does, and runs three camera scenarios (orbit, pan, zoom) of 300 frames each. `synthetic:N` is a generated model of `N` boxes, each its own element, so the numbers do not depend on a private file. `WGPU_BACKEND` chooses the backend.

## Phase 0, 2026-11-06

Commit `1db9e34` with the synthetic model of the commit that adds this file. The reference PC is a laptop with an NVIDIA RTX 2000 Ada Generation Laptop GPU (discrete), Vulkan.

| Model | Triangles | Scenario | fps | p95 (ms) | max (ms) | over 16.67 ms | Criterion |
|---|---:|---|---:|---:|---:|---:|---|
| `synthetic:100000` (100 000 elements) | 1 200 000 | orbit | 749 | 1.85 | 5.53 | 0 | pass |
| | | pan | 741 | 1.43 | 4.60 | 0 | pass |
| | | zoom | 663 | 1.69 | 3.58 | 0 | pass |
| `test-models/test-kavel.skp` | 110 956 | orbit | 4157 | 0.30 | 1.03 | 0 | pass |
| | | pan | 4136 | 0.28 | 1.07 | 0 | pass |
| | | zoom | 3859 | 0.32 | 0.98 | 0 | pass |
| A 194 MB IFC 4 model (21 layers, not in the repository) | 1 660 342 | orbit | 374 | 3.20 | 8.15 | 0 | pass |
| | | pan | 354 | 3.49 | 5.47 | 0 | pass |
| | | zoom | 332 | 4.27 | 6.25 | 0 | pass |

## Phase 1, 2026-11-06

Selection and hiding go through the flags of the units texture, so the bench now uploads the units of every element, as the app does, and adds two scenarios: orbit with every element selected, and orbit with every second element hidden. It also times `Hide` of every element in the app's `update`, against the 8.33 ms of one frame. Same reference PC, branch `feat/selection-visibility`.

| Model | Scenario | fps | p95 (ms) | max (ms) | over 16.67 ms | Criterion |
|---|---|---:|---:|---:|---:|---|
| `synthetic:100000` | orbit | 912 | 1.23 | 2.45 | 0 | pass |
| | pan | 1006 | 1.10 | 2.32 | 0 | pass |
| | zoom | 909 | 1.26 | 2.74 | 0 | pass |
| | orbit, all selected | 906 | 1.37 | 4.15 | 0 | pass |
| | orbit, half hidden | 1268 | 0.87 | 1.91 | 0 | pass |
| | hide 100 000 elements in `update` | | 2.95 | | | pass |
| `test-models/test-kavel.skp` | orbit | 2858 | 0.85 | 1.36 | 0 | pass |
| | orbit, all selected | 2679 | 0.94 | 1.02 | 0 | pass |
| | orbit, half hidden | 2696 | 0.93 | 2.49 | 0 | pass |
| | hide 235 elements in `update` | | 0.03 | | | pass |
| The 194 MB IFC 4 model | orbit | 400 | 3.41 | 6.33 | 0 | pass |
| | orbit, all selected | 350 | 3.67 | 8.31 | 0 | pass |
| | orbit, half hidden | 442 | 2.82 | 4.44 | 0 | pass |
| | hide 1 368 elements in `update` | | 0.23 | | | pass |

The first measurement of hiding 100 000 elements took 8.78 ms and failed. Hashing the keys of the hidden set was most of it; a faster hash (`units::KeyHasher`) brought it to 2.95 ms.

## Reference models and integrated graphics, 2026-11-06

Commit `176114c`, the setup of Phase 1 (units uploaded). `O3V_BENCH_GPU=low` selects the integrated GPU of the same laptop, an Intel RaptorLake-S Mobile Graphics Controller (Vulkan). The reference models now cover every renderer path the spec names:
- `synthetic:100000`;
- `test-models/test-kavel.skp`;
- `37_model.ifc`, 194 MB, IFC 4;
- `Example File Location Analysis.rvt`, 87 MB, 3 075 578 triangles;
- `warmoeskade-047.e57`, a 234 MB scan of 5 124 788 points.

Only the first two are in the repository. The table gives fps and p95 frame time in ms per camera scenario.

| Model | GPU | orbit | pan | zoom | orbit, all selected | orbit, half hidden | Criterion |
|---|---|---|---|---|---|---|---|
| `synthetic:100000` | discrete | 843, 1.44 | 875, 1.41 | 856, 1.21 | 852, 2.01 | 1155, 0.98 | pass |
| | integrated | 44, 29.05 | 47, 23.84 | 39, 27.72 | 43, 31.49 | 146, 8.69 | fail |
| `test-kavel.skp` | discrete | 4169, 0.26 | 4248, 0.27 | 3839, 0.30 | 3926, 0.32 | 3938, 0.32 | pass |
| | integrated | 422, 3.96 | 474, 2.76 | 440, 3.93 | 475, 3.18 | 484, 3.36 | pass |
| `37_model.ifc` | discrete | 364, 4.49 | 363, 4.47 | 331, 5.58 | 376, 5.37 | 458, 4.99 | pass |
| | integrated | 75, 14.55 | 74, 14.37 | 70, 15.61 | 76, 14.10 | 81, 13.50 | fail |
| `Example File Location Analysis.rvt` | discrete | 411, 2.70 | 468, 2.27 | 455, 2.46 | 431, 2.42 | 465, 2.29 | pass |
| | integrated | 60, 20.19 | 61, 21.50 | 63, 17.94 | 65, 16.53 | 65, 17.15 | fail |
| `warmoeskade-047.e57` | discrete | 125, 10.37 | 136, 9.16 | 140, 8.78 | | | **fail** |
| | integrated | 18, 59.78 | 17, 60.81 | 17, 64.35 | | | fail |

- **The point cloud misses the criterion on the reference PC.** The fps are above 120, but the p95 is 8.8 to 10.4 ms, and the orbit and pan runs each have one frame over 16.67 ms. Drawing point clouds needs renderer work, in a plan of its own, as Phase 0 requires.
- **Integrated graphics:** these rows are the baseline for the rule "no regression against the baseline". They were measured with the Phase 1 code; the Phase 0 code was not measured on this GPU.
- **Times in `update` vary with the laptop's state.** Here the times for 100 000 elements were:
  - box select of everything: 16.93 ms (7.16 to 7.61 ms in three earlier runs);
  - isolate one: 10.47 ms (5.38 to 5.66 ms earlier);
  - hide everything: 5.51 ms (2.65 to 2.96 ms earlier).

  On the integrated GPU's run they were 14.56, 10.39 and 5.17 ms. Hiding stays within one frame; box selecting and isolating 100 000 elements take one to two frames. The models of a few thousand elements stay below 0.5 ms. Run the same commands on a computer with integrated graphics only and add its rows here; later phases compare against them.
