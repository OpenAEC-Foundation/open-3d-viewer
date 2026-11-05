# Native product packages

The shared Rust repository builds two independent desktop products:

| Identity | Viewer | Validator |
| --- | --- | --- |
| Display and settings folder | Open 3D Viewer Studio | Open BIM Validator |
| Binary and installer catalogue ID | `open-3d-viewer` | `open-bim-validator` |
| Bundle / Linux desktop ID | `org.openaec.Open3DViewer` | `org.openaec.OpenBIMValidator` |
| Cargo feature | default | `bim-validator` |
| Remote-control environment variable | `O3V_REMOTE` | `OBV_REMOTE` |

Settings, shortcuts, recent files, GPU logs and remote announcements use the product's own folder
under `OpenAEC`. Existing viewer preferences retain their original location. Windows installers
use separate per-user program folders, shortcuts and uninstall registry keys. Uninstalling one
does not remove the other product or either product's preferences. The validator does not ship
the viewer MCP executable.

Build each product in a separate Cargo invocation. Cargo features are unified across a build,
including Windows executable resources; `cargo build --all-features --bins` is therefore not a
two-product release build. The viewer executable and MCP server reject a validator-feature build
at startup. The validator entrypoint requires the validator feature.

```powershell
cargo build --release --locked --bin open-3d-viewer --bin open-3d-viewer-mcp
cargo build --release --locked --features bim-validator --bin open-bim-validator
python scripts/build-products.py --plan
python scripts/build-products.py --output dist/review-2026-45
```

Python 3.11+ is only build tooling; it is not packaged and is not a runtime dependency. The script
builds each identity separately, immediately stages its binaries, and produces distinct archives
with source revision, dirty-state flag, product metadata and SHA-256 hashes. Output must be a new
directory to prevent reuse of stale artifacts. Native platform and architecture are used; this
script does not cross-compile. Linux archives contain `bin/` and `share/` trees that can be installed
under a chosen prefix. macOS archives contain distinct `.app` bundles. Windows archives contain
portable binaries; add `--windows-installers` with NSIS on PATH for distinct per-user installers.

The manual `products.yml` workflow creates **unsigned review artifacts**, not releases. The
manifest records signatures as `not-verified`; hashes do not certify a publisher. The existing
viewer release/signing pipeline remains intact. Before publishing the validator, use verified
Windows signing (including its uninstaller), macOS Developer ID signing and notarization, and
the project's Linux signing policy. The NSIS validator recipe supports the same `SIGN` hook as
the viewer recipe; a failed uninstaller signing command fails packaging. The review script does
not pass that hook or claim signed output.

For the separate credentialed, fail-closed release workflow for both products, see
[Signed native product release route](SIGNED-PRODUCTS.md).

Update notifications query only the active product's installer catalogue ID. There is no fallback
from validator to viewer. Publication requires a separate validator catalogue entry pointing to
validator artifacts; absent that entry, no validator update is advertised. Both products use
native iced/wgpu windows, so these packages do not use Tauri or WebView2.

Run packaging contract tests with:

```powershell
python -m unittest discover -s scripts -p test_build_products.py -v
```
