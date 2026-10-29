# Productvideo: materiaal

Alles om de productvideo van Open 3D Viewer Studio te monteren, ook op een andere computer en met een andere tool of skill. Het materiaal komt uit het echte venster. [`scripts/product_video.py`](../../scripts/product_video.py) heeft het opgenomen door de viewer via zijn eigen MCP-server te bedienen.

## Ophalen

```bash
git clone https://github.com/OpenAEC-Foundation/open-3d-viewer.git
cd open-3d-viewer
git checkout feat/blueprint-shell
```

Zolang [#21](https://github.com/OpenAEC-Foundation/open-3d-viewer/pull/21) niet is samengevoegd, staat het materiaal alleen op de branch `feat/blueprint-shell`. Daarna staat het op `main`.

## Wat er staat

| Bestand | Wat |
|---------|-----|
| [`storyboard.json`](storyboard.json) | Het draaiboek en de bron voor de montage: dertien scènes op volgorde, met begin en duur in seconden, onderschriften in het Engels en het Nederlands, de teksten van de kaarten, links, versie, licentie en huisstijl |
| `clips/*.mp4` | De drie bewegende scènes, 1920x1200, 30 fps, H.264, **zonder onderschrift**: rond het model draaien, het doorsnedekader en uit elkaar halen |
| `stills/*.jpg` | Per scène het laatste beeld, 1920x1200, zonder onderschrift. Voor scènes van het type `still` is dit het hele beeld; voor `card` is het de kaart zoals het script hem tekent |

Per scène in `storyboard.json`:

- `kind`: het type scène.
  - `clip`: een bewegend stuk, uit `clip`.
  - `still`: een stilstaand beeld, uit `still`, dat `duration_s` lang in beeld blijft.
  - `card`: een tekstkaart met `title` en `lines`. Maak de kaart zelf, of gebruik de `still`.
- `caption`: het onderschrift in `en` en `nl`.
- `notes`: wat er te zien is.

De totale lengte is 40 seconden.

## Ander materiaal in de repository

- **Huisstijl:** accentkleur `#D97706` (oranje) en marineblauw `#0F1B2D` voor de kaarten. Het lettertype is Inter: `assets/fonts/Inter-Regular.ttf` en `Inter-SemiBold.ttf`.
- **Logo en app-icoon:** `assets/logo.svg` en `assets/icons/icon.png` (512 px).
- **Schermafbeeldingen van de README:** `docs/images/readme/`, 1440x900. Daarin staan de thema's, de Bestand-pagina, de instellingen, de sneltoetsen, het bovenaanzicht, uit elkaar en de Revit-staat.
- **Links:** de browserversie op https://open-3d-viewer.open-aec.com en de downloads op https://github.com/OpenAEC-Foundation/open-3d-viewer/releases/latest.

## Om te weten

- **Taal van de interface:** de opnames tonen de Engelse interface. Een Nederlandse versie vraagt nieuwe opnames: `--language nl` (zie hieronder).
- **Geen muis te zien:** het script bedient het venster via de MCP-server, dus er is geen cursor en geen klik. Laat een onderschrift of een eigen animatie zeggen wat er gebeurt.
- **Revit-beelden:** die komen uit het Autodesk-voorbeeld `snowdon_arch_2025.rvt` en de Revit-build van de viewer.

## Opnieuw opnemen

Dit vraagt Windows met een GPU, Rust en [uv](https://docs.astral.sh/uv/). Voor de Revit-scènes zijn ook de Revit-build (`revit/`, met de privé-rvt-reader) en een Revit-bestand met een Door Schedule nodig. Zonder die twee slaat het script de Revit-scènes over.

```bash
cargo build --release
uv run --with pillow --with imageio-ffmpeg python scripts/product_video.py --kit --revit snowdon_arch_2025.rvt
```

Daarmee worden `clips/`, `stills/` en `storyboard.json` opnieuw geschreven. Daarnaast komt er een kant-en-klare versie met onderschriften in `target/video/open-3d-viewer.mp4`, die niet in de repository komt. `--language nl` neemt op met de Nederlandse interface. Het script opent een eigen viewervenster en sluit het aan het eind.
