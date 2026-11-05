"""Publication planning tests use mocked commands and never contact GitHub."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("publish_products", Path(__file__).with_name("publish-products.py"))
publish = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publish)


class ProductPublicationTests(unittest.TestCase):
    def assets(self, root):
        for product in publish.PRODUCTS:
            for suffix in ("_2026.45.10_windows-x64.zip", "-SHA256SUMS", "-SHA256SUMS.asc"):
                (root / (product + suffix)).write_bytes(b"fixture")

    def success(self, command, **kwargs):
        return subprocess.CompletedProcess(command, 1, "", "gh: Not Found (HTTP 404)") if command[:2] == ["gh", "api"] else subprocess.CompletedProcess(command, 0)

    def test_two_separate_releases_preflighted_before_either_publication(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(publish.subprocess, "run", side_effect=self.success) as run:
            root = Path(directory) / "assets"
            root.mkdir()
            self.assets(root)
            publish.publish(root, "2026.45.10", "a" * 40, "owner/repo", True)
            commands = [call.args[0] for call in run.call_args_list]
            self.assertTrue(all(command[:2] == ["gh", "api"] for command in commands[2:6]))
            created = commands[6:]
            self.assertEqual(len(created), 2)
            for command, (product, name) in zip(created, publish.PRODUCTS.items()):
                self.assertEqual(command[:4], ["gh", "release", "create", product + "-v2026.45.10"])
                self.assertIn(name + " 2026.45.10", command)
                self.assertIn("a" * 40, command)
                self.assertIn("--prerelease", command)
                other = next(key for key in publish.PRODUCTS if key != product)
                self.assertFalse(any(other in argument for argument in command))
                notes = (root.parent / (product + "-release-notes.md")).read_text()
                self.assertIn(f"`sha256sum -c {product}-SHA256SUMS`", notes)
                (root.parent / (product + "-release-notes.md")).unlink()

    def test_existing_second_tag_prevents_first_publication(self):
        def existing(command, **kwargs):
            if command[:2] == ["gh", "api"] and "open-bim-validator" in command[-1]:
                return subprocess.CompletedProcess(command, 0, "{}", "")
            return self.success(command, **kwargs)
        with tempfile.TemporaryDirectory() as directory, patch.object(publish.subprocess, "run", side_effect=existing) as run:
            root = Path(directory) / "assets"
            root.mkdir()
            self.assets(root)
            with self.assertRaisesRegex(ValueError, "already exists"):
                publish.publish(root, "2026.45.10", "a" * 40, "owner/repo", True)
            self.assertFalse(any(call.args[0][:3] == ["gh", "release", "create"] for call in run.call_args_list))

    def test_second_failure_reports_partial_publication(self):
        def fail_second(command, **kwargs):
            if command[:4] == ["gh", "release", "create", "open-bim-validator-v2026.45.10"]:
                raise subprocess.CalledProcessError(1, command)
            return self.success(command, **kwargs)
        with tempfile.TemporaryDirectory() as directory, patch.object(publish.subprocess, "run", side_effect=fail_second):
            root = Path(directory) / "assets"
            root.mkdir()
            self.assets(root)
            with self.assertRaisesRegex(RuntimeError, "Confirmed earlier releases: open-3d-viewer-v2026.45.10"):
                publish.publish(root, "2026.45.10", "a" * 40, "owner/repo", True)


if __name__ == "__main__":
    unittest.main()
