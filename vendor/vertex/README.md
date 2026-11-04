# vertex.dll

The Vertex BD (`.vxz`, `.vxm`) and BTL (`.btl`) reader as a shared library, from the private
OpenAEC repository `vertex.dll`, release `v0.3.0` (commit `126835d9e4d7f3d6f2330e0d3934b0b7df2e5af5`). The viewer loads it at run
time; the installers put it next to the program. Update with `scripts/update-vertex.sh <tag>`.

| File | Platform |
|---|---|
| `vertex.dll` | Windows x64 |
| `libvertex.so` | Linux x64 |
| `libvertex.dylib` | macOS, Apple silicon and Intel |

Checksums: `SHA256SUMS`. Interface: C, version 1 (`vertex_abi_version`, `vertex_read`, `vertex_free`).
