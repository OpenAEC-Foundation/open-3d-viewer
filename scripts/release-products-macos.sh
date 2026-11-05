#!/usr/bin/env bash
# Developer ID and notarization are mandatory; no ad-hoc fallback.
set -euo pipefail
for name in APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_KEY_CONTENT APPLE_API_KEY APPLE_API_ISSUER RUNNER_TEMP; do
  [[ -n "${!name:-}" ]] || { echo "Required signing setting unavailable: $name" >&2; exit 1; }
done
stage=$1
product_name=$2
dmg=$3
evidence=$4
app="$stage/$product_name.app"
signing_dir=$(mktemp -d "$RUNNER_TEMP/product-signing.XXXXXX")
keychain="$signing_dir/signing.keychain-db"
cleanup() {
  security delete-keychain "$keychain" >/dev/null 2>&1 || true
  rm -f "$signing_dir/id.p12" "$signing_dir/key.p8"
}
trap cleanup EXIT
printf '%s' "$APPLE_CERTIFICATE" | base64 -D > "$signing_dir/id.p12"
if [[ "$APPLE_API_KEY_CONTENT" == *"-----BEGIN"* ]]; then
  printf '%s\n' "$APPLE_API_KEY_CONTENT" > "$signing_dir/key.p8"
else
  printf '%s' "$APPLE_API_KEY_CONTENT" | base64 -D > "$signing_dir/key.p8"
fi
chmod 600 "$signing_dir/id.p12" "$signing_dir/key.p8"
password=$(uuidgen)
security create-keychain -p "$password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
security import "$signing_dir/id.p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
# Set access using the temporary keychain password, never the certificate password.
security set-key-partition-list -S apple-tool:,apple: -s -k "$password" "$keychain"
security find-identity -v -p codesigning "$keychain" | grep -Fq "$APPLE_SIGNING_IDENTITY"
for binary in "$app/Contents/MacOS/"*; do
  codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" --keychain "$keychain" "$binary"
done
codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" --keychain "$keychain" "$app"
codesign --verify --deep --strict --verbose=2 "$app"
codesign -dvv "$app" 2>&1 | grep -q 'Authority=Developer ID Application:'
ln -s /Applications "$stage/Applications"
size=$(( $(du -sm "$stage" | cut -f1) * 2 + 64 ))
hdiutil create -volname "$product_name" -srcfolder "$stage" -size "${size}m" -fs HFS+ -ov -format UDZO "$dmg"
codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" --keychain "$keychain" "$dmg"
codesign --verify --strict --verbose=2 "$dmg"
xcrun notarytool submit "$dmg" --key "$signing_dir/key.p8" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER" --wait --output-format json > "$signing_dir/notary.json"
python3 -c 'import json,sys; assert json.load(open(sys.argv[1]))["status"] == "Accepted", "Notarization rejected"' "$signing_dir/notary.json"
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
python3 - "$signing_dir/notary.json" "$evidence" <<'PY'
import json, sys
from pathlib import Path
notary = json.loads(Path(sys.argv[1]).read_text())
Path(sys.argv[2]).write_text(json.dumps({"method": "developer-id-notarized", "verified": True, "notarization_id": notary["id"], "notarization_status": notary["status"]}) + "\n")
PY
