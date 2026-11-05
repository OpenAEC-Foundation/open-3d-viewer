"""Release inventory gates fail closed before credentials or publication are used."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("release_products", Path(__file__).with_name("release-products.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseInventoryTests(unittest.TestCase):
    def inventory(self, root):
        for product_id, product in release.build.PRODUCTS.items():
            for system in ("windows", "darwin", "linux"):
                stem = f"{product_id}_2026.45.10_{system}-x64"
                suffixes = {"windows": (".zip", "-setup.exe"), "darwin": (".dmg",), "linux": (".tar.gz",)}[system]
                artifacts = {}
                for suffix in suffixes:
                    artifact = root / (stem + suffix)
                    artifact.write_bytes((product_id + system + suffix).encode())
                    artifacts[artifact.name] = release.digest(artifact)
                if system == "windows":
                    files = [binary + ".exe" for binary in product["bins"]] * 2 + [stem + "-setup.exe", "uninstall.exe"]
                    signing = {"method": "authenticode-timestamped", "verified": True,
                               "files": [{"file": name, "subject": "Test Publisher", "thumbprint": "A" * 40} for name in files]}
                elif system == "darwin":
                    signing = {"method": "developer-id-notarized", "verified": True,
                               "notarization_status": "Accepted", "notarization_id": "a5cfbdae-bf81-4d84-8815-05bc6e4de66a"}
                else:
                    signing = {"method": "openpgp-pending", "verified": False}
                metadata = {
                    "product": product_id, "bundle_id": product["bundle_id"], "architecture": "x64",
                    "platform": system, "source_revision": "source-commit", "version": "2026.45.10",
                    "signing": signing, "sha256": artifacts,
                }
                (root / (stem + ".json")).write_text(json.dumps(metadata))

    def test_complete_inventory_has_one_revision_and_version(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.inventory(root)
            self.assertEqual(release.verify_artifacts(root, "source-commit"), "2026.45.10")

    def test_rejects_wrong_revision_unsigned_missing_or_modified_artifacts(self):
        for mutation in ("revision", "signature", "missing", "tamper", "extra", "identity", "wrong_type", "missing_type", "method", "architecture", "uninstaller"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.inventory(root)
                metadata_path = next(root.glob("*windows*.json"))
                metadata = json.loads(metadata_path.read_text())
                artifact = root / next(iter(metadata["sha256"]))
                if mutation == "revision":
                    metadata["source_revision"] = "different-commit"
                elif mutation == "signature":
                    metadata["signing"]["verified"] = False
                elif mutation == "missing":
                    artifact.unlink()
                elif mutation == "tamper":
                    artifact.write_bytes(b"changed")
                elif mutation == "extra":
                    (root / "unlisted.exe").write_bytes(b"unexpected")
                elif mutation == "identity":
                    metadata["bundle_id"] = "wrong-product"
                elif mutation == "wrong_type":
                    renamed = artifact.with_suffix(".tar.gz")
                    artifact.rename(renamed)
                    metadata["sha256"][renamed.name] = metadata["sha256"].pop(artifact.name)
                elif mutation == "missing_type":
                    metadata["sha256"].pop(artifact.name)
                    artifact.unlink()
                elif mutation == "method":
                    metadata["signing"]["method"] = "self-signed"
                elif mutation == "architecture":
                    metadata["architecture"] = "arm64"
                elif mutation == "uninstaller":
                    metadata["signing"]["files"] = [record for record in metadata["signing"]["files"] if record["file"] != "uninstall.exe"]
                metadata_path.write_text(json.dumps(metadata))
                with self.assertRaises((ValueError, FileNotFoundError)):
                    release.verify_artifacts(root, "source-commit")

    def test_macos_requires_dmg_and_accepted_notarization_evidence(self):
        for mutation in ("type", "method", "status", "submission"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.inventory(root)
                path = next(root.glob("*darwin*.json"))
                metadata = json.loads(path.read_text())
                if mutation == "type":
                    name = next(iter(metadata["sha256"]))
                    renamed = name.removesuffix(".dmg") + ".tar.gz"
                    (root / name).rename(root / renamed)
                    metadata["sha256"][renamed] = metadata["sha256"].pop(name)
                elif mutation == "method":
                    metadata["signing"]["method"] = "ad-hoc"
                elif mutation == "status":
                    metadata["signing"]["notarization_status"] = "Rejected"
                else:
                    metadata["signing"].pop("notarization_id")
                path.write_text(json.dumps(metadata))
                with self.assertRaises(ValueError):
                    release.verify_artifacts(root, "source-commit")


if __name__ == "__main__":
    unittest.main()
