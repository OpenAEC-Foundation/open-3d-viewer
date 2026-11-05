# Signed native product release route

`products-release.yml` is a separate manual workflow. It builds both products from the exact
workflow commit SHA, tests both feature profiles and the shared workspace, verifies platform
signatures, signs the final inventory with OpenPGP, and creates GitHub provenance attestations.
The existing viewer `release.yml` is unchanged.

Defaults are `publish=false` and `prerelease=true`. A normal manual run therefore creates
verified downloadable workflow artifacts without creating a release. Setting `publish=true`
creates two releases from the same source SHA: `open-3d-viewer-v<package version>` titled
`Open 3D Viewer Studio <version>`, and `open-bim-validator-v<package version>` titled
`Open BIM Validator <version>`. Each contains only its own packages, manifests, detached
signatures and signed `<product>-SHA256SUMS` inventory. Both destination tags/releases are
checked before publishing either; existing destinations are refused. Stable publication requires explicitly
setting `prerelease=false`. Nothing here changes either product's installer catalogue entry.

Configure the `native-product-release` GitHub environment with the desired release branch
restrictions and required reviewers. Merely declaring an environment in YAML does not configure
those protections. Its secrets may be environment, repository or appropriately shared
organization secrets. Signing credentials are required even for a nonpublishing run.

## Required access and credentials

| Purpose | Existing secret / variable contract |
| --- | --- |
| Private native format dependencies | `FORMAT_CRATES_SSH_KEYS` read-only deploy keys |
| Windows Azure Artifact Signing | `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_ENDPOINT`, `AZURE_TRUSTED_SIGNING_ACCOUNT_NAME`, `AZURE_CERTIFICATE_PROFILE_NAME` secrets |
| macOS Developer ID | `APPLE_CERTIFICATE` base64 PKCS#12, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` secrets |
| Apple notarization | `APPLE_API_KEY_CONTENT` PEM or base64 key, `APPLE_API_KEY` key ID, `APPLE_API_ISSUER` secrets |
| Organization OpenPGP | `OPENAEC_GPG_PRIVATE_KEY`, `OPENAEC_GPG_PASSPHRASE` secrets and `OPENAEC_GPG_FINGERPRINT` variable containing the full 40-digit fingerprint |
| Provenance | GitHub workflow `id-token: write` and `attestations: write`; repository/account must support artifact attestations |
| Optional publication | GitHub workflow token with `contents: write` in the publish job |

Windows uses the existing `scripts/windows-sign.ps1` contract and its pinned signing client.
It signs each executable, requires NSIS 3.08+ to sign the embedded uninstaller, then signs the
installer. A silent installation into a unique runner temporary directory verifies the installed
executables and uninstaller with `Get-AuthenticodeSignature`, including timestamp certificates.
No signing failure is ignored, and there is no retry that drops the signing requirement.

macOS requires a real Developer ID Application identity. Every executable and bundle is signed
with hardened runtime and timestamping. Signature verification precedes DMG construction; the
DMG is signed, submitted to Apple, checked for `Accepted`, stapled and validated. There is no
ad-hoc signing fallback. Temporary key material is removed when the signing script exits.

Linux archives have no native executable signature. Their manifests explicitly record
`openpgp-pending` until the final job verifies the complete six-product/platform inventory and
signs every package and manifest with the exact organization fingerprint. That job also signs
`SHA256SUMS`, verifies all detached signatures and hashes, and attests the final assets.
Platform build artifacts alone are intermediate outputs; only `native-products-verified` is the
complete distribution set. Missing secrets, signatures, notarization, packages, matching source
revisions, checksums or attestations stop the workflow before publication.

## Package scope and verification

Windows produces independent per-user installers and portable ZIP files. macOS produces one
DMG per product for the runner's native architecture; these are not universal binaries. Linux
produces prefix-installable `bin/` and `share/` tar archives, not DEB/RPM/AppImage packages.
Filenames and JSON manifests record the actual architecture. Viewer and validator names,
bundle identifiers, preferences, uninstall entries and update catalogue IDs remain separate.

After downloading the complete release set, verify:

```sh
sha256sum -c open-bim-validator-SHA256SUMS
gpg --verify open-bim-validator-SHA256SUMS.asc open-bim-validator-SHA256SUMS
gh attestation verify PACKAGE --repo OWNER/REPOSITORY
```

Obtain the organization public key/fingerprint through a trusted channel; a signature is only
useful when its identity is trusted. Inspect individual platform JSON manifests for the source
SHA, package hashes, Windows certificate subjects/thumbprints and Apple notarization IDs.

Local contract tests require no credentials and perform no signing or publication:

```sh
python -m unittest discover -s scripts -p 'test_*products.py' -v
```

The workflow and recipes require a real credentialed runner execution before their first
release can be considered verified. Static validation and local contract tests do not establish
that organization credentials are available or that platform services accepted a package.

## Partial-publication recovery

GitHub does not offer an atomic operation spanning two releases. If the second publication fails,
the workflow fails and identifies earlier successful releases. The failed command may itself
have created a draft or release with incomplete assets. Inspect both product tags and asset lists
before recovery. Keep the successful release intact. Download `native-products-verified` from
that exact workflow run, verify its signed checksum inventories and source SHA, and create the
missing product release or complete only its missing assets from that artifact. Do not rebuild,
replace existing assets, or move either tag to a different source revision. A normal workflow
rerun intentionally refuses the already-existing destination; recovery is an explicit maintainer
operation using the preserved verified assets and original prerelease setting.
