#!/bin/bash
# Copies the Vertex BD / BTL reader library (the private OpenAEC repository vertex.dll) into
# vendor/vertex: vertex.dll, libvertex.so and libvertex.dylib from one of its releases, with
# their checksums. Only the compiled library is in this repository; the installers ship it and
# the viewer loads it at run time (src/loaders/vertex.rs). Needs `gh` with access to vertex.dll.
#
#   scripts/update-vertex.sh v0.1.0
set -euo pipefail
tag=${1:?usage: scripts/update-vertex.sh <tag of OpenAEC-Foundation/vertex.dll>}
dir=$(cd "$(dirname "$0")/.." && pwd)/vendor/vertex
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
gh release download "$tag" --repo OpenAEC-Foundation/vertex.dll --dir "$tmp"
(cd "$tmp" && sha256sum -c SHA256SUMS)
mkdir -p "$dir"
cp "$tmp/vertex.dll" "$tmp/libvertex.so" "$tmp/libvertex.dylib" "$tmp/SHA256SUMS" "$dir/"
commit=$(gh api "repos/OpenAEC-Foundation/vertex.dll/commits/$tag" --jq .sha)
cat > "$dir/README.md" <<NOTE
# vertex.dll

The Vertex BD (\`.vxz\`, \`.vxm\`) and BTL (\`.btl\`) reader as a shared library, from the private
OpenAEC repository \`vertex.dll\`, release \`$tag\` (commit \`$commit\`). The viewer loads it at run
time; the installers put it next to the program. Update with \`scripts/update-vertex.sh <tag>\`.

| File | Platform |
|---|---|
| \`vertex.dll\` | Windows x64 |
| \`libvertex.so\` | Linux x64 |
| \`libvertex.dylib\` | macOS, Apple silicon and Intel |

Checksums: \`SHA256SUMS\`. Interface: C, version 1 (\`vertex_abi_version\`, \`vertex_read\`, \`vertex_free\`).
NOTE
ls -la "$dir"
