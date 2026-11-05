"""Packaging contract tests; no compiler, installer or signing credentials required."""

import importlib.util
import json
from pathlib import Path
import plistlib
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("build_products", Path(__file__).with_name("build-products.py"))
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)


class ProductPackagingTests(unittest.TestCase):
    def test_builds_are_separate_and_feature_is_only_on_validator(self):
        viewer = build.cargo_command(build.PRODUCTS["open-3d-viewer"], Path("target"))
        validator = build.cargo_command(build.PRODUCTS["open-bim-validator"], Path("target"))
        self.assertNotIn("--features", viewer)
        self.assertEqual(validator[-2:], ["--features", "bim-validator"])
        self.assertNotIn("open-3d-viewer", validator)
        self.assertIn("--locked", viewer)
        self.assertIn("--locked", validator)

    def test_staged_platform_packages_keep_product_identity_and_hashes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries = root / "release"
            binaries.mkdir()
            for product in build.PRODUCTS.values():
                for binary in product["bins"]:
                    for suffix in ("", ".exe"):
                        (binaries / (binary + suffix)).write_bytes(binary.encode())
            for system in ("Windows", "Linux", "Darwin"):
                for product_id, product in build.PRODUCTS.items():
                    with self.subTest(platform=system, product=product_id):
                        stage = root / system / product_id
                        build.stage_product(product_id, stage, binaries, system, "2026.45.10")
                        if system == "Darwin":
                            contents = stage / (product["name"] + ".app") / "Contents"
                            metadata = plistlib.loads((contents / "Info.plist").read_bytes())
                            self.assertEqual(metadata["CFBundleIdentifier"], product["bundle_id"])
                            self.assertEqual(metadata["CFBundleExecutable"], product_id)
                            self.assertEqual(metadata["CFBundleShortVersionString"], "2026.45.10")
                        if system == "Linux":
                            desktop = stage / "share/applications" / (product["bundle_id"] + ".desktop")
                            self.assertIn("Exec=" + product_id + " %f", desktop.read_text())
                        build.write_manifest(stage, product_id, "2026.45.10", "test-revision", True, system, "x86_64")
                        manifest = json.loads((stage / "artifact.json").read_text())
                        self.assertEqual(manifest["product"], product_id)
                        self.assertEqual(manifest["signature"], "not-verified")
                        self.assertTrue(manifest["sha256"])
                        other_id = "open-bim-validator" if product_id == "open-3d-viewer" else "open-3d-viewer"
                        self.assertFalse(any(other_id in name for name in manifest["sha256"]))

    def test_windows_installer_namespaces_do_not_overlap(self):
        for product_id, product in build.PRODUCTS.items():
            recipe = (build.ROOT / "packaging/windows" / product["installer"]).read_text()
            self.assertIn('InstallDir "$LOCALAPPDATA\\Programs\\' + product["name"] + '"', recipe)
            self.assertIn('Uninstall\\' + product["name"] + '"', recipe)
            self.assertIn('Delete "$INSTDIR\\' + product_id + '.exe"', recipe)
            other_id = "open-bim-validator" if product_id == "open-3d-viewer" else "open-3d-viewer"
            self.assertNotIn(other_id, recipe)


if __name__ == "__main__":
    unittest.main()
