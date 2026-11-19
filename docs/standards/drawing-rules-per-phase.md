# What a Dutch architectural drawing shows per phase

The table the view templates of the Library are made from (`crates/bim-library/src/template.rs`,
issue #309). Read on 19 November 2026. Each fact names its source and how sure it is:

- **read**: the text was read on the page named.
- **listed**: seen in a catalogue or a search summary of the page, not read in full.
- **practice**: common practice in Dutch offices; no source was found for it. These are the
  suite's choices and are an office's to change.

The standards themselves (NEN 47, NEN 2574, the ISO series) are paid documents and were not
read; only their catalogue pages were.

## Phases

The phases and their abbreviations are those of the Standaardtaakbeschrijving DNR-STB 2014
(read: [toelichting](https://www.dekempenaer.nl/content/uploads/2015/07/standaardtaakbeschrijving_2014_toelichting.pdf)):
01 initiatief/haalbaarheid, 02 projectdefinitie, 03 structuurontwerp (SO, optional),
04 voorontwerp (VO), 05 definitief ontwerp (DO), 06 technisch ontwerp (TO), 07 prijs- en
contractvorming, 08 uitvoeringsgereed ontwerp (UO), 09 directievoering, 10 gebruik.

The permit application is no phase of the STB. It is usually filed from the definitief
ontwerp (practice), and the law sets what its drawings show; the templates have it as a phase
of its own between DO and TO.

A newer edition (STB 2026, alpha) exists and was not read (listed:
[BNA](https://bna.nl/wp-content/uploads/2026/06/20260617-Toelichting-Handleiding-STB-2026-alfaversie.pdf)).

## Scales and content per phase

| Phase | Plans, sections, elevations | Details | What the drawings show | Source |
|---|---|---|---|---|
| SO | massing and layout plan 1:500 / 1:200 | none stated | main mass, layout of functions, access | read: [STB fase 3](https://www.dekempenaer.nl/content/uploads/2015/07/stb_fase_3_structuurontwerp_1_mei_2014.pdf) |
| VO | 1:200 / 1:100; site 1:500 | none stated | place of functions, area and function per room, space kept for structure, services and shafts; sections in at least two directions; elevations with materials named broadly | read: [STB fase 4](https://dekempenaer.nl/content/uploads/2015/07/stb_fase_4_voorontwerp_1_mei_2014.pdf) |
| DO | plans and sections 1:100 / 1:200; elevations 1:200 / 1:100; fragments 1:50; site 1:500 | principal details 1:5 | fixed furnishing, room names, area per room, fire compartments, place and size of building parts | read: [STB fase 5](https://www.dekempenaer.nl/content/uploads/2015/07/stb_fase_5_definitief_ontwerp_1_mei_2014.pdf) |
| Permit | not smaller than 1:100 (1:200 from 10 000 m² gross floor area); site not smaller than 1:1000 | 1:5, 1:10 or 1:20 | see below | read: [Omgevingsregeling art. 7.17](https://wetten.overheid.nl/BWBR0045528/2024-01-01/0/Hoofdstuk7/Afdeling7.2/Paragraaf7.2.2/Artikel7.17) |
| TO | 1:100 / 1:200 with dimensions and specifications of rooms and building parts; fragments 1:20 / 1:50; site 1:200 / 1:500 | principal details 1:5 / 1:1 | full dimensioning, specifications, finishes schedule | read: [STB fase 6](https://www.dekempenaer.nl/content/uploads/2015/07/stb_fase_6_technisch_ontwerp_bestek_1_mei_2014.pdf) |
| UO | construction drawings 1:50 (also 1:100, 1:20); shape drawings 1:50 / 1:20 | 1:5; production 1:5 / 1:1 | place and size of everything to be built | read: [STB fase 8](https://bna.nl/wp-content/uploads/2019/06/STB_fase08_Uitv-Uitvoeringsgereed_ontwerp_01mei2014.pdf) |

Where sources and practice differ: many offices draw technical-design plans at 1:50. The STB
has them at 1:100 / 1:200 with fragments at 1:20 / 1:50, and the templates follow the STB.

## The permit application

The Omgevingsregeling (in force since 1 January 2024; the version of that date was read,
later changes were not checked):

- **Art. 7.17**: the scales above; a north arrow; dimensions and the scale stated.
- **Art. 7.18**: a plan is a cut at 1200 mm above the floor. It shows the separating
  constructions with their materials named, levels, stairs and ramps, frames, shafts, and
  the areas of the functions of use; floor levels against street level and ground level at
  the entrance; plans and sections with dimensions and heights; elevations in orthogonal
  projection.
- **Art. 7.10**: functions of use, habitable areas, room sizes and occupancy, total areas
  per function, floor levels.

Read: [art. 7.17 and following](https://wetten.overheid.nl/BWBR0045528/2024-01-01/0/Hoofdstuk7/Afdeling7.2/Paragraaf7.2.2/Artikel7.17),
[IPLO overview](https://iplo.nl/regelgeving/regels-voor-activiteiten/technische-bouwactiviteit/plichten/vergunning-bouwactiviteit).

Materials and colours of the exterior (facades, pointing, frames, windows and doors,
balustrades, gutters, fascias, roofing) are asked by the municipality's omgevingsplan, not by
the Omgevingsregeling; a municipality may change this (read in one plan:
[Elburg, art. 22.35](https://lokaleregelgeving.overheid.nl/CVDR696231/5)). The former Regeling
omgevingsrecht (repealed 1 January 2024) had the same scales in art. 2.8 and the same plan
content in art. 2.9 (read: [Mor](https://wetten.overheid.nl/BWBR0027471/2023-07-01)).

## Detail per scale

No source was found that says how much of an element a drawing shows at each scale. The
templates follow common practice:

| Scale | Walls, floors, roofs | Doors and windows | Certainty |
|---|---|---|---|
| 1:200 and smaller | outline, filled in one tone | an opening | practice |
| 1:100 | one hatch for the whole element | frame and swing, simple | practice |
| 1:50 | every layer with its own hatch | frame with its parts | practice |
| 1:20 to 1:5 | all of it, with its parts and fixings | all of it | practice |

A teaching text gives the use of scales (1:1 to 1:5 detail, 1:20 working drawing, 1:50
working and specification drawing, 1:100 design, 1:500 / 1:1000 site) and says a section
"stays open" where the material is too small to hatch at the scale (read:
[Wikiwijs](https://maken.wikiwijs.nl/bestanden/570020/Tekenen%20en%20tekening%20lezen.pdf)).
Another lists what a DO sheet shows: grid lines, main dimensions, room names, hatches, door
swings, the section line, a legend and a title block (read:
[Wikiwijs](https://maken.wikiwijs.nl/bestanden/1151715/Wat%20staat%20er%20op%20een%20DO%20tekening1.pdf)).

Line weights per scale were not found: the page of TU Delft that offices cite says only that
its sheet is largely based on NEN 47, and its sheet could not be reached. The pens of the
suite's Dutch drawing standard (0.5 / 0.25 mm up to 1:50, 0.35 / 0.18 mm beyond) are practice.

## Standards

| Standard | Title | Status | Certainty |
|---|---|---|---|
| NEN 47:1970 | Technische tekeningen - Doorsnede-aanduidingen van materialen op bouwkundige tekeningen | current | read: [NEN](https://www.nen.nl/nen-47-1970-nl-7779) |
| NEN 2574:1993 | Tekeningen in de bouw - Indeling van gegevens op tekeningen voor gebouwen (lists of what a drawing holds at least, per phase) | current | read: [NEN](https://www.nen.nl/nen-2574-1993-nl-8163) |
| NEN 2302:1983 | Tekeningen in de bouw - Algemene regels | withdrawn 27 March 2026, no successor named | read: [NEN](https://www.nen.nl/nen-2302-1983-nl-7876) |
| NEN 2580:2007 | Oppervlakten en inhouden van gebouwen | current | read: [NEN](https://www.nen.nl/nen-2580-2007-nl-113982) |
| ISO 7519:2025 | General arrangement and assembly drawings | current | listed: [ISO](https://www.iso.org/standard/89718.html) |
| ISO 128-2:2022 | Basic conventions for lines | current | listed: [ISO](https://www.iso.org/standard/69129.html) |
| ISO 4157-1:1998 | Designation systems | current | listed: [ISO](https://www.iso.org/standard/26189.html) |
| ISO 13567-1:2017 | Organization and naming of layers for CAD | current | listed: [ISO](https://www.iso.org/standard/70181.html) |
| ISO 7200, 5457, 3098, 129 | Title blocks, sheet sizes, lettering, dimensioning | reported current | listed |

NEN 2574 is the standard closest to this table. It was not read; an office that has it should
lay it beside the templates.

## BIM basis ILS

Version 2 (2 October 2020) was read from a copy, not from bimloket.nl
([copy](https://www.han.nl/opleidingen/post-hbo/bim-digitaal-data-beheer/bim-BIM-basis-ILS-versie-2.pdf)):
a four-digit NL-SfB code (3.6), `LoadBearing` (4.3), `IsExternal` (4.4), `FireRating` (4.5), an
`IfcMaterial` on every object (4.7), rooms as `IfcSpace` (4.1).

## What the templates take from this

- A template per phase for floor plans, sections and elevations, at the first scale the STB or
  the law names for that phase; details at 1:5 from the DO on.
- Detail by scale, as the table of practice above.
- Rooms from the VO; fixed furnishing and grid lines from the DO; dimensions and levels from
  the DO, as the permit asks them; services faint from the TO.
- The permit template is the DO template at a scale not smaller than 1:100, with dimensions,
  levels and materials named.
