#!/usr/bin/env bash
# Final signing gate: exact organization fingerprint, packages and checksum inventory.
set -euo pipefail
for name in OPENAEC_GPG_PRIVATE_KEY OPENAEC_GPG_PASSPHRASE OPENAEC_GPG_FINGERPRINT RUNNER_TEMP; do
  [[ -n "${!name:-}" ]] || { echo "Required signing setting unavailable: $name" >&2; exit 1; }
done
assets=$1
export GNUPGHOME
GNUPGHOME=$(mktemp -d "$RUNNER_TEMP/product-gpg.XXXXXX")
chmod 700 "$GNUPGHOME"
trap 'gpgconf --kill gpg-agent; rm -f "$GNUPGHOME/passphrase"' EXIT
printf '%s\n' "$OPENAEC_GPG_PRIVATE_KEY" | gpg --batch --import
fingerprint=$(printf '%s' "$OPENAEC_GPG_FINGERPRINT" | tr -d '[:space:]' | tr '[:lower:]' '[:upper:]')
[[ "$fingerprint" =~ ^[0-9A-F]{40}$ ]] || { echo 'A full OpenPGP fingerprint is required' >&2; exit 1; }
gpg --batch --with-colons --list-secret-keys "$fingerprint" | awk -F: '$1 == "fpr" {print $10}' | grep -Fxq "$fingerprint"
printf '%s' "$OPENAEC_GPG_PASSPHRASE" > "$GNUPGHOME/passphrase"
chmod 600 "$GNUPGHOME/passphrase"
# Sign every package and source/signature manifest, including Linux tar archives.
for file in "$assets/"*; do
  [[ -f "$file" && "$file" != *.asc && "$file" != */SHA256SUMS ]] || continue
  gpg --batch --yes --pinentry-mode loopback --passphrase-file "$GNUPGHOME/passphrase" --local-user "$fingerprint" --armor --detach-sign --output "$file.asc" "$file"
  gpg --batch --verify "$file.asc" "$file"
done
# Each product release has a self-contained checksum inventory and signature.
for product in open-3d-viewer open-bim-validator; do
  (cd "$assets" && sha256sum -- "${product}_"* > "${product}-SHA256SUMS")
  checksums="$assets/${product}-SHA256SUMS"
  gpg --batch --yes --pinentry-mode loopback --passphrase-file "$GNUPGHOME/passphrase" --local-user "$fingerprint" --armor --detach-sign --output "$checksums.asc" "$checksums"
  gpg --batch --verify "$checksums.asc" "$checksums"
  (cd "$assets" && sha256sum -c "${product}-SHA256SUMS")
done
(cd "$assets" && sha256sum -- * > SHA256SUMS)
gpg --batch --yes --pinentry-mode loopback --passphrase-file "$GNUPGHOME/passphrase" --local-user "$fingerprint" --armor --detach-sign --output "$assets/SHA256SUMS.asc" "$assets/SHA256SUMS"
gpg --batch --verify "$assets/SHA256SUMS.asc" "$assets/SHA256SUMS"
(cd "$assets" && sha256sum -c SHA256SUMS)
