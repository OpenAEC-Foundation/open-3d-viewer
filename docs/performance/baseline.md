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

**Integrated graphics:** not measured yet. Run the same commands on a computer with integrated graphics only and add its rows here; later phases compare against them.
