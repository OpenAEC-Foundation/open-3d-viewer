#!/usr/bin/env python3
"""Publish two distinct product releases from already verified workflow assets."""
import os
from pathlib import Path
import re
import subprocess
import sys

PRODUCTS = {"open-3d-viewer": "Open 3D Viewer Studio", "open-bim-validator": "Open BIM Validator"}


def release_notes(product, name, version, revision, repository):
    return f"""{name} {version}, built from {revision}.

This product has its own binaries, settings, installer and update ID (`{product}`). Windows packages have verified timestamped Authenticode signatures including the uninstaller. macOS disk images are Developer ID signed, notarized and stapled. Linux archives and manifests have verified organization OpenPGP signatures. GitHub build-provenance attestations cover the distributed assets.

Artifact names record their native architecture; macOS packages are not universal. Linux packages are prefix-installable tar archives. Product/platform JSON manifests contain source identity, hashes and signing evidence. This release does not change the installer catalogue.

Verify this release with `sha256sum -c {product}-SHA256SUMS`, `gpg --verify {product}-SHA256SUMS.asc {product}-SHA256SUMS`, and `gh attestation verify FILE --repo {repository}`. Capability limitations remain documented in the source migration ledger; publication does not imply complete migration parity.
"""


def publish(assets, version, revision, repository, prerelease):
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version) or not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("A package version and exact source SHA are required")
    plans = []
    for product, name in PRODUCTS.items():
        tag = f"{product}-v{version}"
        files = sorted(assets.glob(product + "_*"))
        checksums = [assets / (product + "-SHA256SUMS"), assets / (product + "-SHA256SUMS.asc")]
        if not files or any(not path.is_file() for path in files + checksums):
            raise ValueError("Missing product assets or signed checksums: " + product)
        subprocess.run(["sha256sum", "-c", checksums[0].name], cwd=assets, check=True)
        plans.append((product, name, tag, files + checksums))
    # Preflight both destinations before the first publication. Fail on API/auth errors too.
    for _, _, tag, _ in plans:
        for endpoint in (f"repos/{repository}/git/ref/tags/{tag}", f"repos/{repository}/releases/tags/{tag}"):
            result = subprocess.run(["gh", "api", endpoint], capture_output=True, text=True)
            if result.returncode == 0:
                raise ValueError("Release or tag already exists: " + tag)
            if "HTTP 404" not in result.stderr:
                raise RuntimeError("Cannot verify release destination: " + result.stderr.strip())
    created = []
    for product, name, tag, files in plans:
        notes = assets.parent / (product + "-release-notes.md")
        notes.write_text(release_notes(product, name, version, revision, repository))
        command = ["gh", "release", "create", tag, "--repo", repository, "--target", revision,
                   "--title", f"{name} {version}", "--notes-file", str(notes)]
        if prerelease:
            command.append("--prerelease")
        try:
            subprocess.run(command + [str(path) for path in files], check=True)
        except subprocess.CalledProcessError as error:
            raise RuntimeError(
                f"Publication failed for {tag}. Confirmed earlier releases: {', '.join(created) or 'none'}. "
                "The failed command may also have created a partial release. Inspect both destinations; "
                "recover the missing release/assets from this run's native-products-verified artifact "
                f"at source {revision}. Do not replace an existing release or rebuild its artifacts."
            ) from error
        created.append(tag)


if __name__ == "__main__":
    try:
        publish(Path("release-assets"), os.environ["VERSION"], os.environ["GITHUB_SHA"],
                os.environ["GITHUB_REPOSITORY"], os.environ["PRERELEASE"] == "true")
    except (ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
