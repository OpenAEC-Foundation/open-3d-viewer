# Approved application logos

The PNG masters are the approved Snap Store logos. `icons.json` records their source URLs, SHA-256 checksums and all generated application assets. Transparency and the approved artwork are preserved.

Regenerate with Python 3 and Pillow installed:

```sh
python3 scripts/generate-approved-icons.py
```

Do not redraw these logos or replace them with the foundation symbol. Historical screenshots and unrelated third-party branding are not application icon sources.

The web app (`web/icons/`) and the Android launcher (`packaging/android/.../mipmap-*`) are generated from the same master. Their maskable and adaptive variants are flattened onto the tile colour (`#E89F17`, also `iconBackground` in `packaging/android/app/src/main/res/values/colors.xml`) so the platform mask never shows the tile's edge; the lettering stays inside each platform's safe zone.
