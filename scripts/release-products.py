#!/usr/bin/env python3
"""Prepare native products and package only after platform signing verification."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import tarfile
import tomllib
import zipfile
import uuid
from collections import Counter

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("build_products", ROOT / "scripts/build-products.py")
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)
STAGING = ROOT / "dist/product-release-staging"
OUTPUT = ROOT / "dist/product-release"


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def source_metadata():
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if revision != os.environ.get("GITHUB_SHA"):
        raise ValueError("Release builds must use the exact workflow source revision")
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip():
        raise ValueError("Release builds require a clean source checkout")
    return {
        "source_revision": revision,
        "version": tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"],
        "platform": platform.system().lower(),
        "architecture": {"amd64": "x64", "x86_64": "x64", "aarch64": "arm64"}.get(platform.machine().lower(), platform.machine().lower()),
    }


def prepare():
    metadata = source_metadata()
    STAGING.mkdir(parents=True, exist_ok=False)
    OUTPUT.mkdir(parents=True, exist_ok=False)
    target = ROOT / "target/product-release"
    for product_id, product in build.PRODUCTS.items():
        subprocess.run(build.cargo_command(product, target), cwd=ROOT, check=True)
        build.stage_product(product_id, STAGING / product_id, target / "release", platform.system(), metadata["version"])
    (STAGING / "build.json").write_text(json.dumps(metadata, indent=2) + "\n")


def package():
    metadata = json.loads((STAGING / "build.json").read_text())
    if metadata != source_metadata():
        raise ValueError("Source or platform changed after preparation")
    system = platform.system()
    for product_id, product in build.PRODUCTS.items():
        stage = STAGING / product_id
        stem = f"{product_id}_{metadata['version']}_{metadata['platform']}-{metadata['architecture']}"
        evidence = STAGING / (product_id + "-signatures.json")
        if system == "Windows":
            subprocess.run(["pwsh", "-NoProfile", "-NonInteractive", "-File",
                            str(ROOT / "scripts/release-products-windows.ps1"),
                            "-Product", product_id, "-Stage", str(stage), "-Output", str(OUTPUT),
                            "-Version", metadata["version"], "-Stem", stem, "-Evidence", str(evidence)], check=True)
            with zipfile.ZipFile(OUTPUT / (stem + ".zip"), "w", zipfile.ZIP_DEFLATED) as archive:
                for path in sorted(stage.rglob("*")):
                    if path.is_file():
                        archive.write(path, path.relative_to(stage.parent))
        elif system == "Darwin":
            subprocess.run(["bash", str(ROOT / "scripts/release-products-macos.sh"),
                            str(stage), product["name"], str(OUTPUT / (stem + ".dmg")), str(evidence)], check=True)
        elif system == "Linux":
            with tarfile.open(OUTPUT / (stem + ".tar.gz"), "w:gz") as archive:
                archive.add(stage, arcname=product_id)
            evidence.write_text(json.dumps({"method": "openpgp-pending", "verified": False}))
        else:
            raise ValueError("Unsupported platform")
        signing = json.loads(evidence.read_text())
        if system != "Linux" and signing.get("verified") is not True:
            raise ValueError("Platform signature verification did not succeed")
        artifacts = {path.name: digest(path) for path in sorted(OUTPUT.glob(stem + ".*"))}
        # Windows installers use the same stem plus a setup suffix.
        artifacts.update({path.name: digest(path) for path in sorted(OUTPUT.glob(stem + "-setup.exe"))})
        if not artifacts:
            raise ValueError("No package was produced")
        manifest = dict(metadata, product=product_id, bundle_id=product["bundle_id"], signing=signing, sha256=artifacts)
        (OUTPUT / (stem + ".json")).write_text(json.dumps(manifest, indent=2) + "\n")


def verify_artifacts(assets, expected_revision):
    """Fail before signing/publication if any product/platform package is missing or changed."""
    seen = set()
    version = None
    listed = set()
    architectures = {}
    for path in sorted(assets.glob("*.json")):
        manifest = json.loads(path.read_text())
        key = (manifest["product"], manifest["platform"])
        if key in seen or manifest["product"] not in build.PRODUCTS:
            raise ValueError("Unexpected or duplicate product manifest")
        if manifest["bundle_id"] != build.PRODUCTS[manifest["product"]]["bundle_id"]:
            raise ValueError("Incorrect product bundle identity")
        if manifest["source_revision"] != expected_revision:
            raise ValueError("Products were built from different source revisions")
        version = version or manifest["version"]
        if version != manifest["version"] or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
            raise ValueError("Product version mismatch")
        system = manifest["platform"]
        architecture = manifest.get("architecture")
        supported = {"windows": {"x64"}, "linux": {"x64"}, "darwin": {"x64", "arm64"}}
        if architecture not in supported.get(system, set()):
            raise ValueError("Unsupported platform architecture")
        if architectures.setdefault(system, architecture) != architecture:
            raise ValueError("Products must use the same architecture per platform")
        stem = f"{manifest['product']}_{version}_{system}-{architecture}"
        suffixes = {"windows": {".zip", "-setup.exe"}, "linux": {".tar.gz"}, "darwin": {".dmg"}}
        if path.name != stem + ".json" or set(manifest["sha256"]) != {stem + suffix for suffix in suffixes[system]}:
            raise ValueError("Incorrect product artifact filenames or package types")
        signing = manifest["signing"]
        if system == "windows":
            if signing.get("method") != "authenticode-timestamped" or signing.get("verified") is not True:
                raise ValueError("Missing timestamped Authenticode verification")
            records = signing.get("files", [])
            expected_files = Counter({binary + ".exe": 2 for binary in build.PRODUCTS[manifest["product"]]["bins"]})
            expected_files.update({stem + "-setup.exe": 1, "uninstall.exe": 1})
            if Counter(record.get("file") for record in records) != expected_files:
                raise ValueError("Missing installed payload or uninstaller signature evidence")
            if any(not record.get("subject") or not re.fullmatch(r"[0-9a-fA-F]{40}", record.get("thumbprint", "")) for record in records):
                raise ValueError("Missing Authenticode signer identity")
        elif system == "darwin":
            if signing.get("method") != "developer-id-notarized" or signing.get("verified") is not True or signing.get("notarization_status") != "Accepted":
                raise ValueError("Missing accepted Developer ID notarization")
            try:
                uuid.UUID(signing.get("notarization_id", ""))
            except (ValueError, AttributeError) as error:
                raise ValueError("Missing notarization submission identifier") from error
        elif signing.get("method") != "openpgp-pending" or signing.get("verified") is not False:
            raise ValueError("Linux must proceed through the final OpenPGP signing gate")
        for name, expected in manifest["sha256"].items():
            if Path(name).name != name or name in listed:
                raise ValueError("Unsafe or duplicate artifact path")
            if digest(assets / name) != expected:
                raise ValueError("Artifact checksum mismatch: " + name)
            listed.add(name)
        seen.add(key)
    expected = {(product, system) for product in build.PRODUCTS for system in ("windows", "linux", "darwin")}
    if seen != expected:
        raise ValueError("Both products must have Windows, Linux and macOS artifacts")
    actual = {p.name for p in assets.iterdir() if p.is_file() and p.suffix != ".json"}
    if actual != listed:
        raise ValueError("Unlisted files in release assets")
    return version


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["prepare", "package", "verify"])
    parser.add_argument("--assets", type=Path, default=Path("release-assets"))
    args = parser.parse_args()
    if args.action == "prepare":
        prepare()
    elif args.action == "package":
        package()
    else:
        print(verify_artifacts(args.assets, os.environ["GITHUB_SHA"]))
