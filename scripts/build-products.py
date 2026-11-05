#!/usr/bin/env python3
"""Build separate native product artifacts; development tooling, never shipped."""

import argparse
import hashlib
import json
import platform
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PRODUCTS = {
    "open-3d-viewer": {
        "name": "Open 3D Viewer Studio",
        "bundle_id": "org.openaec.Open3DViewer",
        "features": [],
        "bins": ["open-3d-viewer", "open-3d-viewer-mcp"],
        "plist": "Info.plist.in",
        "installer": "installer.nsi",
    },
    "open-bim-validator": {
        "name": "Open BIM Validator",
        "bundle_id": "org.openaec.OpenBIMValidator",
        "features": ["bim-validator"],
        "bins": ["open-bim-validator"],
        "plist": "ValidatorInfo.plist.in",
        "installer": "validator.nsi",
    },
}


def cargo_command(product, target_dir):
    command = ["cargo", "build", "--release", "--locked", "--no-default-features",
               "--target-dir", str(target_dir)]
    for binary in product["bins"]:
        command.extend(["--bin", binary])
    if product["features"]:
        command.extend(["--features", ",".join(product["features"])])
    return command


def stage_product(product_id, stage, release_dir, system, version):
    product = PRODUCTS[product_id]
    if system == "Darwin":
        contents = stage / (product["name"] + ".app") / "Contents"
        binary_dir = contents / "MacOS"
        resources = contents / "Resources"
        resources.mkdir(parents=True)
        shutil.copy2(ROOT / "assets/icons/icon.icns", resources / "AppIcon.icns")
        template = (ROOT / "packaging/macos" / product["plist"]).read_text()
        (contents / "Info.plist").write_text(template.replace("@NUMBER@", version))
    else:
        binary_dir = stage / "bin" if system == "Linux" else stage
    binary_dir.mkdir(parents=True, exist_ok=True)
    for binary in product["bins"]:
        filename = binary + (".exe" if system == "Windows" else "")
        shutil.copy2(release_dir / filename, binary_dir / filename)
    if system == "Linux":
        applications = stage / "share/applications"
        applications.mkdir(parents=True)
        desktop = product["bundle_id"] + ".desktop"
        shutil.copy2(ROOT / "packaging/linux" / desktop, applications / desktop)
        icons = stage / "share/icons/hicolor/128x128/apps"
        icons.mkdir(parents=True)
        shutil.copy2(ROOT / "assets/icons/128x128.png", icons / (product["bundle_id"] + ".png"))
    notices = stage / "licenses/openaec-ui"
    notices.mkdir(parents=True)
    shutil.copy2(ROOT / "vendor/openaec-ui/LICENSE.md", notices / "LICENSE.md")
    shutil.copy2(ROOT / "vendor/openaec-ui/NOTICE.md", notices / "NOTICE.md")
    shutil.copytree(ROOT / "vendor/openaec-ui/licenses", notices / "licenses")
    shutil.copy2(ROOT / "assets/fonts/Inter-LICENSE.txt", stage / "licenses/Inter-LICENSE.txt")
    for filename in ("LICENSE", "THIRD-PARTY-NOTICES.md"):
        if (ROOT / filename).is_file():
            shutil.copy2(ROOT / filename, stage / filename)


def write_manifest(stage, product_id, version, revision, dirty, system, architecture):
    files = {
        path.relative_to(stage).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(stage.rglob("*")) if path.is_file()
    }
    manifest = {
        "product": product_id, "name": PRODUCTS[product_id]["name"],
        "bundle_id": PRODUCTS[product_id]["bundle_id"], "version": version,
        "source_revision": revision, "source_dirty": dirty,
        "platform": system, "architecture": architecture,
        "distribution": "review-artifact", "signature": "not-verified",
        "sha256": files,
    }
    (stage / "artifact.json").write_text(json.dumps(manifest, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--product", choices=["all", *PRODUCTS], default="all")
    parser.add_argument("--output", type=Path, default=ROOT / "dist/products")
    parser.add_argument("--plan", action="store_true", help="Print build commands without changing files")
    parser.add_argument("--windows-installers", action="store_true", help="Also build unsigned NSIS installers")
    args = parser.parse_args()
    selected = PRODUCTS if args.product == "all" else [args.product]
    target_dir = ROOT / "target/products"
    if args.plan:
        print(json.dumps([cargo_command(PRODUCTS[p], target_dir) for p in selected], indent=2))
        return
    system = platform.system()
    if system not in ("Windows", "Linux", "Darwin"):
        parser.error("Unsupported native packaging platform")
    if args.windows_installers and (system != "Windows" or not shutil.which("makensis")):
        parser.error("--windows-installers requires Windows and makensis on PATH")
    output = args.output.resolve()
    # Refuse stale output, preventing an older artifact from being mistaken for this build.
    output.mkdir(parents=True, exist_ok=False)
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip())
    architecture = platform.machine().lower()
    for product_id in selected:
        product = PRODUCTS[product_id]
        subprocess.run(cargo_command(product, target_dir), cwd=ROOT, check=True)
        with tempfile.TemporaryDirectory(prefix=product_id + "-") as temporary:
            stage = Path(temporary) / product_id
            stage_product(product_id, stage, target_dir / "release", system, version)
            if args.windows_installers:
                installer = output / f"{product_id}_{version}_{architecture}-unsigned-setup.exe"
                subprocess.run(["makensis", f"/DNUMBER={version}", f"/DSOURCE={stage}",
                                f"/DOUTPUT={installer}",
                                str(ROOT / "packaging/windows" / product["installer"])], check=True)
            write_manifest(stage, product_id, version, revision, dirty, system, architecture)
            name = f"{product_id}_{version}_{system.lower()}_{architecture}-review"
            if system == "Windows":
                with zipfile.ZipFile(output / (name + ".zip"), "w", zipfile.ZIP_DEFLATED) as archive:
                    for path in sorted(stage.rglob("*")):
                        if path.is_file():
                            archive.write(path, path.relative_to(stage.parent))
            else:
                with tarfile.open(output / (name + ".tar.gz"), "w:gz") as archive:
                    archive.add(stage, arcname=product_id)
    hashes = {path.name: hashlib.sha256(path.read_bytes()).hexdigest()
              for path in sorted(output.iterdir()) if path.is_file()}
    (output / "SHA256SUMS").write_text("".join(f"{digest}  {name}\n" for name, digest in hashes.items()))


if __name__ == "__main__":
    main()
