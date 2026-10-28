# Reusable format crates

The upstream repositories below own these crates. This viewer carries source snapshots
so desktop builds, web deployment and CI work without private GitHub credentials.
No parser implementation remains in the viewer for SKP, DB1 or PLY.
The original viewer API re-exports the shared types to keep external loaders compatible.

| Repository | Pinned commit |
| --- | --- |
| [openaec-model](https://github.com/OpenAEC-Foundation/openaec-model) | `392747b39bb56721aeac0f007002e6ea6b332eb6` |
| [skp-reader](https://github.com/OpenAEC-Foundation/skp-reader) | `adda39237b598f0bbdcf9baa9628d9038ea60db4` |
| [tekla-reader](https://github.com/OpenAEC-Foundation/tekla-reader) | `fa9581559cccc93f87a5a4844872815c65fdfa10` |
| [ply-reader](https://github.com/OpenAEC-Foundation/ply-reader) | `80e1447d132c95fed304010c626bf786801f0586` |

The only source overlay is in each reader's Cargo.toml: its pinned Git dependency on
openaec-model becomes `path = "../openaec-model"`, keeping `version = "0.1.0"`.
That ensures all readers and the viewer use the same Rust types.
`formats-lock.json` records SHA-256 hashes of the installed snapshot, including this overlay.
Licenses travel with each crate. The SKP port also retains openskp's MIT notice.
Revit stays in the existing private rvt-dll repository and its current crates; glTF,
OBJ and STL already use the existing gltf, tobj and stl_io crates.

## Updating a snapshot

1. Make parser/model changes in its upstream repository and test them there.
2. Export the desired full commit with `git archive`, copying the crate files into its
   vendor directory. Keep large sample models and build output out of the snapshot.
3. For reader manifests, replace only the openaec-model Git URL and revision with the
   sibling path shown above. Update the model snapshot to the revision the readers use.
4. Record the upstream commit and refreshed file hashes in formats-lock.json and this table.
5. Run `python scripts/check_format_snapshots.py`, the standalone crate tests, corpus
   tests, viewer tests and web checks. Commit the snapshots and metadata together.

## Validation

```sh
python scripts/check_format_snapshots.py
cargo test --manifest-path vendor/openaec-model/Cargo.toml
cargo test --manifest-path vendor/skp-reader/Cargo.toml
cargo test --manifest-path vendor/tekla-reader/Cargo.toml
cargo test --manifest-path vendor/ply-reader/Cargo.toml
SKP_TEST_MODELS="$PWD/test-models" cargo test --manifest-path vendor/skp-reader/Cargo.toml --features corpus-tests
cargo test --release --locked
cargo check --locked --target wasm32-unknown-unknown --lib --bin open-3d-viewer
```

In PowerShell, set `$env:SKP_TEST_MODELS = (Resolve-Path test-models).Path` before the
corpus command. The Format crates workflow runs these checks and checks each crate
for wasm32. The optional ignored survey test still uses SKP_DIR.

## Other applications

Applications with read access can depend directly on the reader Git repositories, pinned
to commits, plus the same openaec-model Git URL and revision used in their manifests.
Read files or byte buffers using `Loader::load` or `openaec_model::load_with_loader`.
No viewer, windowing library or GPU is needed. SKP and DB1 retain the documented partial
format coverage of the original implementation; this extraction does not expand support.
