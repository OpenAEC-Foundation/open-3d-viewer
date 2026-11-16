# vertex.dll

The Vertex BD (`.vxz`, `.vxm`) and BTL (`.btl`) reader as a shared library, from the private
OpenAEC repository `vertex.dll`, release `v0.6.0` (commit `b4ab0b135d136050e296a0cf68f8a074666d359c`). The viewer loads it at run
time; the installers put it next to the program. Update with `scripts/update-vertex.sh <tag>`.

| File | Platform |
|---|---|
| `vertex.dll` | Windows x64 |
| `libvertex.so` | Linux x64 |
| `libvertex.dylib` | macOS, Apple silicon and Intel |

Checksums: `SHA256SUMS`. Interface: C, version 1 (`vertex_abi_version`, `vertex_read`, `vertex_free`).
