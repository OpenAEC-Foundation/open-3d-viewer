# Classic SketchUp test files

Small pre-2021 (MFC archive) `.skp` files for the iced-app SketchUp reader tests
(`iced-app/src/loaders/skp/mod.rs`). All are from MIT-licensed repositories, copied unchanged:

| File | SketchUp version | Source |
| --- | --- | --- |
| `openskp_legacy_v3_textured.skp` | 3 | [openskp](https://github.com/iamahsanmehmood/openskp) `packages/cpp/tests/fixtures/legacy_v3_textured.skp` (MIT, Ahsan Mehmood) |
| `openskp_legacy_v4_textured.skp` | 4 | openskp `packages/cpp/tests/fixtures/legacy_v4_textured.skp` |
| `openskp_legacy_v5_textured.skp` | 5 | openskp `packages/cpp/tests/fixtures/legacy_v5_textured.skp` |
| `openskp_nesting_v6.skp` | 6 | openskp `packages/cpp/tests/fixtures/nesting_v6.skp` |
| `openskp_zero_material_custom_layer_v18.skp` | 2018 | openskp `packages/cpp/tests/fixtures/zero_material_custom_layer_v18.skp` |
| `openskp_sdk_saved_2020.skp` | 2020 (saved through SketchUp's C API) | openskp `packages/cpp/tests/fixtures/sdk_saved_2020.skp` |
| `testup_skp-v6.skp` | 6 | [SketchUp/testup-2](https://github.com/SketchUp/testup-2) `tests/SketchUp Ruby API/TC_Sketchup/skp-v6.skp` (MIT, Trimble) |
| `quadface_uv-arch_v8.skp` | 8 | [thomthom/quadface-tools](https://github.com/thomthom/quadface-tools) `Test Models/UV Arch.skp` (MIT, Thomas Thomassen) |

The expected triangle counts and sizes in the test are openskp 1.4.0's results for these files.
