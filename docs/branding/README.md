# Application logo

The logo of Open BIM Suite is three storeys on the amber tile: one for each of its products. Its
source is `assets/logo.svg`; `open-3d-viewer.png` here is that drawing at 1024 px, the master all
the icons are made from. `icons.json` records its SHA-256 checksum and every generated asset.
The snap release checks that checksum, so a new master goes in together with its checksum.

The Snap Store still shows the logo that was uploaded there (`source_url` in `icons.json`); a new
logo is uploaded in the publisher dashboard.

Regenerate the bitmaps from the master with Python 3 and Pillow installed:

```sh
python3 scripts/generate-approved-icons.py
```

The web app (`web/icons/`) and the Android launcher (`packaging/android/.../mipmap-*`) are made
from the same drawing. Their maskable and adaptive variants fill the whole square with the tile
(the script flattens them onto `#E78A08`, also `iconBackground` in
`packaging/android/app/src/main/res/values/colors.xml`), so the platform mask never shows the
tile's edge; the mark stays inside each platform's safe zone.
