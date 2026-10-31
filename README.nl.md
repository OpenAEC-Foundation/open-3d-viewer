[English](README.md) · **Nederlands**

<div align="center">

# Open 3D Viewer Studio

**Een gratis 3D-viewer voor bouwmodellen: hij opent IFC, Revit, SketchUp, Tekla, Allplan, DWG/DXF, CityJSON, LandXML, glTF, OBJ, STL en PLY, meet en doorsnijdt ze, en slaat ze op als IFC, op de desktop en in de browser.**

[![Release](https://img.shields.io/github/v/release/OpenAEC-Foundation/open-3d-viewer?include_prereleases&label=release&color=D97706)](https://github.com/OpenAEC-Foundation/open-3d-viewer/releases/latest)
[![Build](https://github.com/OpenAEC-Foundation/open-3d-viewer/actions/workflows/build.yml/badge.svg)](https://github.com/OpenAEC-Foundation/open-3d-viewer/actions/workflows/build.yml)
[![Releaseworkflow](https://github.com/OpenAEC-Foundation/open-3d-viewer/actions/workflows/release.yml/badge.svg)](https://github.com/OpenAEC-Foundation/open-3d-viewer/actions/workflows/release.yml)
[![Licentie](https://img.shields.io/badge/license-LGPL--3.0-blue)](#licentie)

### [Probeer in je browser](https://open-3d-viewer.open-aec.com) &nbsp;·&nbsp; [Download](https://github.com/OpenAEC-Foundation/open-3d-viewer/releases/latest) &nbsp;·&nbsp; [Bekijk de video](https://www.youtube.com/watch?v=cxHO8M8W2Ow)

[![De productvideo van Open 3D Viewer Studio op YouTube: een model wordt geopend, gedraaid, opengesneden, uit elkaar gehaald, gemeten en als IFC opgeslagen](https://img.youtube.com/vi/cxHO8M8W2Ow/maxresdefault.jpg)](https://www.youtube.com/watch?v=cxHO8M8W2Ow)

*▶ De productvideo (73 s, Engels) op het [kanaal van de OpenAEC Foundation](https://www.youtube.com/@OpenAECFoundation).*

</div>

## Wat deze repository is

Deze repository is de volledige broncode van **Open 3D Viewer Studio**, de 3D-viewer van de [OpenAEC Foundation](https://github.com/OpenAEC-Foundation). Architecten, constructeurs en bouwers sturen elkaar modellen in allerlei formaten; met deze viewer open je ze allemaal zonder het programma waarmee ze gemaakt zijn. Je loopt om het model heen, klikt een element aan en leest zijn eigenschappen, meet, snijdt het open, trekt het uit elkaar per verdieping of per element, en slaat het op als IFC.

Eén Rust-codebasis levert drie dingen: de **desktopapp** voor Windows, macOS en Linux, de **browserversie** op [open-3d-viewer.open-aec.com](https://open-3d-viewer.open-aec.com), en de **MCP-server** waarmee AI-agents de desktopapp bedienen. Daarnaast staat hier wat van die code ondertekende installers en een live website maakt.

| Map | Wat erin staat |
|-----|----------------|
| [`src/`](src/) | De viewer: het venster ([`shell.rs`](src/shell.rs)), de app-toestand ([`app.rs`](src/app.rs)), de 3D-weergave ([`viewport/`](src/viewport/)), meten, doorsnedes, uit elkaar halen, en de afstandsbediening voor de MCP-server |
| [`src/loaders/`](src/loaders/) | Eén adapter per bestandsformaat; [`src/export/`](src/export/) schrijft IFC 4 |
| [`src/bin/open-3d-viewer-mcp.rs`](src/bin/open-3d-viewer-mcp.rs) | De MCP-server voor AI-agents |
| [`vendor/openaec-ui/`](vendor/openaec-ui/), [`assets/b2/`](assets/b2/) | De gedeelde OpenAEC-vensterschil en iconen, overgenomen uit [OpenAEC Blueprint](https://github.com/OpenAEC-Foundation/OpenAEC-Blueprint) 2026.44.1 |
| [`packaging/`](packaging/) | De Windows-installer, de macOS-appbundel en de Linux-pakketten |
| [`.github/workflows/`](.github/workflows/) | `build.yml` test elke wijziging, `release.yml` bouwt en ondertekent een release, `live.yml` publiceert de browserversie |
| [`scripts/`](scripts/) | De README-beelden, de kopie van de B2-iconen en de productvideo, allemaal met een script gemaakt |
| [`test-models/`](test-models/) | De modellen die de tests openen |

De lezers voor SketchUp, Tekla en PLY, en de Revit-bibliotheek, staan in eigen OpenAEC-repositories ([`skp-reader`](https://github.com/OpenAEC-Foundation/skp-reader), [`tekla-reader`](https://github.com/OpenAEC-Foundation/tekla-reader), [`ply-reader`](https://github.com/OpenAEC-Foundation/ply-reader), [`rvt-dll`](https://github.com/OpenAEC-Foundation/rvt-dll)). Deze repository legt elk vast op een vaste commit.

![Een SketchUp-model van een huis op zijn kavel in de viewer, met de modellen en lagen links en de bestandseigenschappen rechts, thema Licht](docs/images/readme/hero.png)

## Probeer het

**[Open de browserversie](https://open-3d-viewer.open-aec.com)**. Sleep een model op de pagina of kies **Openen**, en draai dan met de linkermuisknop, verschuif met de middelste knop en zoom met het wiel. Je hebt een browser met WebGPU nodig: een recente Chrome, Edge, Firefox of Safari. Je bestand blijft op je computer; er wordt niets geüpload.

## Download

Elke release staat op de [releasepagina](https://github.com/OpenAEC-Foundation/open-3d-viewer/releases/latest). Het makkelijkst is de [OpenAEC Installer](https://github.com/OpenAEC-Foundation/openaec-installer): die installeert, werkt bij en start elke OpenAEC-tool.

| Platform | Bestanden |
|----------|-----------|
| **Windows** | `.exe`-installer (x64, zonder beheerdersrechten) of `.zip` |
| **macOS** | `.dmg` (universeel: Apple silicon en Intel) |
| **Linux** | `.deb`, `.rpm`, `.AppImage` of `.tar.gz` |

Als de OpenAEC Installer een nieuwere versie heeft, meldt de titelbalk van de viewer dat, en **Bijwerken** geeft het over aan de installer. Elk bestand is ondertekend: Windows met Authenticode, macOS met een Developer ID en genotariseerd door Apple, Linux met de OpenPGP-sleutel van OpenAEC (een `.asc` naast elk pakket en naast `SHA256SUMS`), en alles met een build-attestatie van GitHub. De release notes zeggen per platform of het ondertekend is; een release waar een handtekening ontbreekt, staat als pre-release.

Zelf een download controleren:

```bash
sha256sum -c SHA256SUMS --ignore-missing
gpg --import docs/openaec-releases.asc   # OpenAEC Foundation Releases, vingerafdruk 13F6 D414 ADEE 4ED4 CD09 AC60 601D 86C9 3C50 4D29
gpg --verify SHA256SUMS.asc SHA256SUMS
gh attestation verify open-3d-viewer_2026.44.19_amd64.deb --repo OpenAEC-Foundation/open-3d-viewer
```

## Waarom gebruiken?

- **Vrij en open.** LGPL-3.0. Geen licentiekosten, geen account, geen upload.
- **Opent wat je toegestuurd krijgt.** IFC en Revit op de desktop; SketchUp (versie 3 en later), Tekla Structures, Allplan, DWG/DXF, CityJSON, LandXML, glTF, OBJ, STL en PLY op de desktop en in de browser.
- **Meerdere modellen tegelijk.** **Openen** vervangt het model, **Toevoegen** zet er een ander bestand naast, en elk model heeft een eigen zichtbaarheidsknop.
- **Slaat op als IFC.** Elk model dat de viewer opent, kan als IFC 4 worden opgeslagen, met verdiepingen, klassen, eigenschappen en materialen.
- **Dezelfde app in de browser en op de desktop.** Eén codebasis; de browserversie hoef je niet te installeren.
- **Meten en doorsnijden.** Afstanden, hoeken, oppervlakten en polygonen, een doorsnedekader en een doorsnedevlak met gevulde snedes, en een uit-elkaar-weergave.
- **AI-agents kunnen hem besturen.** Via een MCP-server openen Claude en andere agents bestanden, kijken rond, selecteren en meten, in het venster dat jij ziet.

## Schermafbeeldingen

De schermafbeeldingen tonen de Engelse interface; in **Instellingen** zet je de taal op Nederlands.

| | |
|---|---|
| ![Het huismodel in de vier thema's Licht, Donker, Blueprint en High contrast donker](docs/images/readme/themes.png) **Licht, Donker, Blueprint en High contrast donker** | ![De kavel van boven als draadmodel met een orthografische camera, thema Donker](docs/images/readme/plan.png) **Bovenaanzicht, draadmodel en orthografisch** |
| ![Een badkamermodel uit elkaar getrokken per element](docs/images/readme/explode.png) **Uit elkaar per element** | ![De Bestand-pagina met Openen, Toevoegen, Opslaan als, Instellingen en de recente bestanden](docs/images/readme/file.png) **De Bestand-pagina met recente bestanden** |
| ![Het instellingenvenster met thema, werkruimte, statusbalk, interfacegrootte en taal, thema Blueprint](docs/images/readme/settings.png) **Instellingen, thema Blueprint** | ![De pagina sneltoetsen in de instellingen, waar elke opdracht een eigen toets kan krijgen](docs/images/readme/shortcuts.png) **Eigen sneltoetsen voor elke opdracht** |

## Bekijken en navigeren

- **Draaien, verschuiven en zoomen.** Slepen met links draait, slepen met de middelste knop verschuift, het wiel zoomt naar de cursor. De kijkkubus draait het model naar een zijde; **F** past het model in beeld.
- **Standaardaanzichten.** Iso, boven, voor en rechts (`0`, `1`, `2`, `3`), perspectief of orthografisch (`P`, `O`), en de opgeslagen aanzichten van een Revit-model.
- **Weergaven.** Volledig, draadmodel en volledig met randen (`S`, `W`, `E`), het grondraster (`G`) en **Z-as omdraaien** voor bestanden met de andere as omhoog.
- **Modellen, lagen en eigenschappen.** De verkenner toont de geopende modellen en hun lagen (IFC-presentatielagen of -klassen, Revit-categorieën, SketchUp-tags, Tekla-klassen, CAD-lagen) met hun kleur en aantal; een klik op een element toont zijn eigenschappen. IFC-ruimtes beginnen verborgen in een eigen laag.
- **Sneltoetsen.** Elke opdracht staat één keer in één lijst, die het lint, de titelbalk en het toetsenbord gebruiken. In **Instellingen** geef je elke opdracht je eigen toets.

## Meten, doorsnijden en uit elkaar halen

- **Meet** afstanden, hoeken, polygonen en oppervlakten door in het model te klikken, of tussen twee elementen. Metingen blijven in beeld tot je ze wist.
- **Doorsnedekader en doorsnedevlak** snijden het model open, met gevulde snedes. Het kader kan zich aanpassen aan de selectie.
- **Uit elkaar** trekt het model per verdieping of per element uit elkaar, met een percentage per as.

## Revit

De desktopapp opent Revit-bestanden (`.rvt`, `.rfa`) via de Revit-bibliotheek [`rvt-dll`](https://github.com/OpenAEC-Foundation/rvt-dll) van OpenAEC. Hij toont de niveaus, stramienen, ruimtes, opgeslagen aanzichten en staten van het bestand. Een staat opent in een eigen tabblad als tabel onder de 3D-weergave, en een rij selecteert de bijbehorende elementen. Revit bewaart van een staat alleen de definitie, dus de viewer rekent de rijen uit de elementen uit; cellen die hij nog niet kan bepalen, tonen `?`.

![Een Revit-model met de Door Schedule als tabel onder de 3D-weergave, thema Donker](docs/images/readme/schedule.png)

De beelden op deze pagina maakt [`scripts/build_readme_images.py`](scripts/build_readme_images.py), dat het echte venster rendert met `open-3d-viewer --screenshot`. Het beeld van de staat gebruikt het Autodesk-voorbeeld `snowdon_arch_2025.rvt`.

## Uitwisseling

| Formaat | Openen | Opslaan |
|---------|--------|---------|
| **IFC 4** (`.ifc`) | Desktop | Ja |
| **Revit** (`.rvt`, `.rfa`) | Desktop | Nee |
| **SketchUp** (`.skp`, versie 3 en later) | Ja | Nee |
| **Tekla Structures** (`.db1`, bèta) | Ja | Nee |
| **Allplan** (`.ndw`-tekenbestanden, bèta) | Ja, 3D-lichamen | Nee |
| **DWG / DXF** (`.dwg`, `.dxf`) | Ja, polygoongeometrie | Nee |
| **CityJSON** (`.json`, `.cityjson`, `.jsonl`) | Ja | Nee |
| **LandXML** (`.xml`, `.landxml`, TIN-oppervlakken) | Ja | Nee |
| **glTF** (`.gltf`, `.glb`) | Ja | Nee |
| **Wavefront OBJ** (`.obj`) | Ja | Nee |
| **STL** (`.stl`, ASCII en binair) | Ja | Nee |
| **PLY** (`.ply`) | Ja | Nee |

Wat niet meekomt:

- **IFC-export** schrijft elk element als driehoeken (`IfcTriangulatedFaceSet`), niet als parametrische geometrie. Klassen komen uit de Revit-categorie of de Tekla-onderdeelnaam, anders `IfcBuildingElementProxy`; elementen uit een geopend IFC-bestand houden hun klasse nog niet. Gecontroleerd met IfcOpenShell 0.8 op Revit-, SketchUp-, OBJ-, STL- en glTF-modellen.
- **DWG en DXF** tonen de polygoongeometrie van de modelruimte. ACIS-volumes, annotaties, curves en externe verwijzingen vallen weg en worden geteld.
- **LandXML** toont TIN-oppervlakken; meetpunten en alignementen worden geen oppervlak. **CityJSON** toont per object het hoogste detailniveau.
- **Texturen** van glTF, OBJ en SketchUp worden hun gemiddelde kleur; zachte randen in SketchUp worden niet gladgemaakt.
- **Tekla**-profielen zijn benaderingen; de lezer is geschreven vanuit een cleanroom-specificatie.
- **Allplan** opent één tekenbestand (`.ndw`) tegelijk en toont de 3D-lichamen (wanden, vloeren, kolommen, ramen, trappen) in één kleur; openingen en ruimtes beginnen verborgen in een eigen laag. Macro's (3D-meubilair), kleuren en materialen, 2D-elementen, hele projectmappen en lay-outs (`.npl`) worden nog niet gelezen. De lezer is geschreven vanuit een cleanroom-specificatie.
- **In de browser** openen IFC en Revit niet, moeten `.gltf`-bestanden hun buffers ingebed hebben, en worden OBJ-materialen niet gelezen.

Nieuwe formaten: implementeer `loaders::Loader` en voeg hem toe aan `loaders::registry()`; nieuwe schrijvers implementeren `export::Writer` en gaan in `export::registry()`.

## MCP

`open-3d-viewer-mcp` is een [MCP](https://modelcontextprotocol.io)-server waarmee een AI-agent (Claude Code, Claude Desktop of een andere MCP-client) de desktopviewer bedient zoals een mens dat doet: bestanden openen, schermafbeeldingen maken, de camera en de standaard- en opgeslagen aanzichten instellen, lagen tonen, verbergen en isoleren, elementen zoeken op eigenschap, selecteren, eigenschappen lezen, een doorsnedekader of -vlak zetten, het model uit elkaar trekken, de weergave wisselen, Revit-staten lezen en meten.

- **Je ziet wat hij doet.** Opdrachten gaan langs dezelfde weg als jouw klikken, in het venster op je scherm. Metingen blijven in beeld.
- **Het blijft op je computer.** Het venster luistert alleen op `127.0.0.1`, met een willekeurig token in `%APPDATA%\OpenAEC\Open 3D Viewer Studio\remote.json`. `--no-remote` of `O3V_REMOTE=0` zet het uit.
- **Installeren:** zet `open-3d-viewer-mcp` naast de viewer (de installers doen dat) en registreer hem, bijvoorbeeld `claude mcp add open-3d-viewer -- "C:\pad\naar\open-3d-viewer-mcp.exe"`. `open-3d-viewer-mcp --print-config` geeft de JSON voor andere clients.

## Testen en betrouwbaarheid

- **119 unittests** in `src/` (118 draaien, 1 vraagt een lokaal voorbeeld), plus 2 voor de MCP-server en 56 voor de vensterbibliotheek in `vendor/openaec-ui`, draaien bij elke pull request en voor elke release: 42 voor de lezers, 14 voor de app, 11 voor meten, 8 voor de vensterschil, 7 voor de doorsnedes, 7 voor IFC-export, en meer voor de 3D-weergave, kijkkubus, afstandsbediening, het model en de uit-elkaar-weergave.
- **De vensterschil wordt getest zonder scherm.** Eén test legt elke weergave op en faalt als een bedieningselement geen naam heeft voor een schermlezer; andere controleren dat geen twee opdrachten dezelfde sneltoets hebben en dat elke opdracht in het lint en de titelbalk bestaat.
- **Gegenereerde bestanden worden gecontroleerd.** `python scripts/build_readme_images.py --check` faalt als een beeld ontbreekt, ongebruikt is of groter is dan 150 kB; `python scripts/b2_icons.py <Blueprint> --check` faalt als een icoon afwijkt van de OpenAEC Blueprint-release.
- **IFC-export** is gecontroleerd met IfcOpenShell 0.8 (schema en where-regels, ruimtelijke indeling, unieke GlobalIds, geometrie) op Revit-voorbeelden van 456 en 6773 elementen, 11 SketchUp-testmodellen, OBJ, STL en glTF.
- **Een release wordt pas gebouwd als de tests slagen**, en de release notes zeggen per platform of de bestanden ondertekend zijn.

Bron van de getallen: `cargo test --release` en `cargo test --release -p openaec-ui --lib`; de release draait dezelfde controles in [`release.yml`](.github/workflows/release.yml).

## Huidige beperkingen

- **IFC en Revit openen alleen op de desktop.** De browserversie heeft die lezers niet.
- **Een aantal formaten ontbreekt.** Puntenwolken (E57, LAS), FBX, 3DS, Collada, 3MF, STEP en IGES openen niet. PLY-bestanden zonder vlakken worden geweigerd.
- **Nog geen ongedaan maken.** Metingen en de doorsnede wis je met de hand.
- **De browserversie blokkeert tijdens het laden** van een groot model (nog geen worker-threads) en onthoudt je instellingen niet tussen bezoeken.
- **Vijf iconen zijn tijdelijk.** De OpenAEC-iconenset heeft nog geen iconen voor volledig, draadmodel, randen, perspectief en orthografisch; de viewer gebruikt de dichtstbijzijnde.
- **Geen loopmodus of verborgen-lijnenweergave.**

Iets gevonden? [Maak een issue](https://github.com/OpenAEC-Foundation/open-3d-viewer/issues).

## Onder de motorkap

De viewer is geschreven in Rust. De interface is [iced](https://iced.rs) 0.14 en de 3D-weergave is een eigen wgpu-pipeline met 4x MSAA in de `shader`-widget van iced, zodat dezelfde code native draait en als WebAssembly op WebGPU, zonder webview of JavaScript-framework. Het venster volgt de OpenAEC-standaardapplicatie: schil, ontwerptokens, lettertypen en B2-iconen komen uit [OpenAEC Blueprint](https://github.com/OpenAEC-Foundation/OpenAEC-Blueprint) 2026.44.1 (`vendor/openaec-ui`, `assets/b2`). IFC wordt gelezen met [IFClite](https://github.com/LTplus-AG/ifc-lite) en DWG/DXF met [acadrust](https://github.com/HakanSeven12/acadrust). Op een computer met geïntegreerde AMD-grafische kaart laadt een Revit-voorbeeld van 18,8 MB in ongeveer 2,5 s en tekent het met ongeveer 110 beelden per seconde op 1920x1080 (`open-3d-viewer --bench`).

## Bijdragen

- **Probeer het op een echt project** en meld fouten of ontbrekende functies als [issue](https://github.com/OpenAEC-Foundation/open-3d-viewer/issues/new). Beschrijf wat je deed, wat je verwachtte en wat er gebeurde, en voeg het bestand toe als je het mag delen.
- **Deel testbestanden.** Modellen die verkeerd openen helpen het meest; `test-models/` bevat de modellen die de tests gebruiken.
- **Schrijf code** en open een pull request. `cargo test --release` moet slagen.

## Snel aan de slag (ontwikkelaars)

```bash
git clone https://github.com/OpenAEC-Foundation/open-3d-viewer.git
cd open-3d-viewer
cargo run --release -- test-models/test-kavel.skp
```

Vereist Rust 1.89 of later (met de MSVC-toolchain op Windows) en leestoegang tot de privé-formaatrepositories van OpenAEC hierboven; zet `CARGO_NET_GIT_FETCH_WITH_CLI=true` zodat Cargo je Git-inloggegevens gebruikt. De browserversie vraagt ook `rustup target add wasm32-unknown-unknown` en `cargo install --locked trunk`; daarna serveert `trunk serve --release` hem op `http://127.0.0.1:8080`. De scripts in `scripts/` vragen Python 3.

De opdrachtregel:

```
open-3d-viewer [BESTAND] [--theme light|dark|blueprint|high-contrast-dark|high-contrast-light] [--language en|nl]
open-3d-viewer [BESTAND] --screenshot UIT.png [--size 1440x900] [--mode solid|wireframe|edges] [--view iso|top|front|right]
               [--ortho] [--page workspace|file|settings|shortcuts] [--schedule NAAM] [--explode X,Y,Z]
open-3d-viewer BESTAND --save-as UIT.ifc
open-3d-viewer --bench BESTAND [--frames 300]
```

Versies volgen het OpenAEC-schema `JJJJ.W.N`: jaar, ISO-week zonder voorloopnul, en een volgnummer binnen de week dat bij 0 begint, zoals `2026.44.19`. Een tag `vJJJJ.W.N` start [`release.yml`](.github/workflows/release.yml), dat de release test, bouwt en ondertekent voor Windows, macOS en Linux en hem publiceert.

Open 3D Viewer Studio hoort bij de familie van apps van de [OpenAEC Foundation](https://github.com/OpenAEC-Foundation).

## Licentie

De eigen broncode van de viewer is LGPL-3.0-or-later. Zie [LICENSE](LICENSE). Het lettertype Inter valt onder de SIL Open Font License (`assets/fonts/Inter-LICENSE.txt`); `vendor/openaec-ui` en zijn lettertypen en iconen staan beschreven in [`NOTICE.md`](vendor/openaec-ui/NOTICE.md). IFClite en acadrust vallen onder MPL-2.0. De desktopapp linkt de Revit-bibliotheek `rvt-dll` van OpenAEC, die haar eigen propriëtaire licentie houdt, dus de desktopbuilds zijn niet helemaal open source; de browserversie bevat haar niet.

## Merken

SketchUp is een merk van Trimble Inc., Revit en DWG van Autodesk, Inc., Tekla Structures van Trimble Solutions Corporation, en Allplan van ALLPLAN GmbH (Nemetschek Group). Open 3D Viewer Studio is een onafhankelijk project en is niet aan hen verbonden, door hen goedgekeurd of gesponsord.
