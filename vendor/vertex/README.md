# vertex.dll

The Vertex BD (`.vxz`, `.vxm`) and BTL (`.btl`) reader as a shared library, from the private
OpenAEC repository `vertex.dll`, release `v0.7.0` (commit `19dbe1ee97e82bb4797931a403244c1936ad0aab`). The viewer loads it at run
time; the installers put it next to the program. Update with `scripts/update-vertex.sh <tag>`.

| File | Platform |
|---|---|
| `vertex.dll` | Windows x64 |
| `libvertex.so` | Linux x64 |
| `libvertex.dylib` | macOS, Apple silicon and Intel |

Checksums: `SHA256SUMS`. Interface: C, version 1 (`vertex_abi_version`, `vertex_read`, `vertex_free`).
