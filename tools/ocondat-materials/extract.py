#!/usr/bin/env python3
"""Extracts the materials of Project Ocondat for `bim-library`.

    python3 tools/ocondat-materials/extract.py PATH/TO/project-ocondat/BlenderBIM/Library.xlsx COMMIT

writes `crates/bim-library/data/ocondat-materials.json`: every material of the sheet
`materials`, each with its key, its names in four languages, its colour, its hatch kind and
the key of its hatch in the hatch catalogue (`data/hatches-nl.json`).

- The sheet names a material by a base, a type and a grade ("hout", "vuren", "C18"). A
  material of one kind that the hatch catalogue has a hatch for (a species of wood, a kind of
  board, of insulation, of stone, of ground) takes that hatch and its names; the others are
  named here.
- The sheet's sign of NEN 47 gives the hatch kind and, where the catalogue has the sign, the
  hatch. Signs 5, 18, 19 and 24 are solid fills the catalogue does not hold: such a material
  has its hatch kind only.
- The sheet puts every species of wood but spruce under hardwood (sign 13), softwoods too, and
  gravel under asphalt. The catalogue has the species under sign 12 and gravel as sign 27, and
  is followed.
- A material the starter library has keeps that library's key, so it is the same material.
"""
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import xlsx  # noqa: E402

source, commit = sys.argv[1], sys.argv[2]
root = Path(__file__).resolve().parents[2]
hatches = json.loads((root / "crates/bim-library/data/hatches-nl.json").read_text())["hatches"]

# the sheet's type, as the hatch catalogue names that kind in Dutch
ALIAS = {
    "vuren_CLT": "vuren CLT", "vuren_gewolmaniseerd": "vuren, verduurzaamd", "red_cedar": "red cedar", "niove": "niové", "azobe": "azobé", "bamboe": "bamboe",
    "okoume": "okoumé", "watervast": "watervaste plaat", "rode_spaan": "rode spaanplaat", "groene_spaan": "groene spaanplaat", "kooltherm": "resolschuim",
    "EPS100": "EPS 100", "tuf": "tufsteen", "poriso": "poreuze baksteen", "rood": "rode baksteen", "helder": "helder glas", "mat": "mat glas",
}
# (base, type, grade) named here: key, nl, en, de, fr
NAMED = {
    ("onbekend", "", ""): ("unknown", "onbekend", "unknown", "unbekannt", "inconnu"),
    ("generiek", "", ""): ("generic", "algemeen", "generic", "allgemein", "générique"),
    ("beton", "", ""): ("concrete", "beton", "concrete", "Beton", "béton"),
    ("beton", "prefab", ""): ("precast-concrete", "prefab beton", "precast concrete", "Betonfertigteil", "béton préfabriqué"),
    ("beton", "tpg", ""): ("concrete-in-situ", "beton, in het werk gestort", "concrete, cast in situ", "Ortbeton", "béton coulé en place"),
    ("staal", "", ""): ("steel", "staal", "steel", "Stahl", "acier"),
    ("staal", "corten", ""): ("weathering-steel", "cortenstaal", "weathering steel", "wetterfester Stahl", "acier autopatinable"),
    ("staal", "thermisch_verzinkt", ""): ("galvanised-steel", "thermisch verzinkt staal", "hot-dip galvanised steel", "feuerverzinkter Stahl", "acier galvanisé à chaud"),
    ("staal", "RVS", ""): ("stainless-steel", "roestvast staal", "stainless steel", "Edelstahl", "acier inoxydable"),
    ("staal", "gietijzer", ""): ("cast-iron", "gietijzer", "cast iron", "Gusseisen", "fonte"),
    ("betonstaal", "", ""): ("reinforcing-steel", "betonstaal", "reinforcing steel", "Betonstahl", "acier d'armature"),
    ("metaal", "", ""): ("metal", "metaal", "metal", "Metall", "métal"),
    ("metaal", "messing", ""): ("brass", "messing", "brass", "Messing", "laiton"),
    ("metaal", "zink", ""): ("zinc", "zink", "zinc", "Zink", "zinc"),
    ("metaal", "lood", ""): ("lead", "lood", "lead", "Blei", "plomb"),
    ("metaal", "koper", ""): ("copper", "koper", "copper", "Kupfer", "cuivre"),
    ("metaal", "aluminium", ""): ("aluminium", "aluminium", "aluminium", "Aluminium", "aluminium"),
    ("hout", "", ""): ("timber", "hout", "timber", "Holz", "bois"),
    ("hout", "naald", ""): ("softwood", "naaldhout", "softwood", "Nadelholz", "bois résineux"),
    ("hout", "loof", ""): ("hardwood", "loofhout", "hardwood", "Laubholz", "bois feuillu"),
    ("hout_plaatmateriaal", "", ""): ("timber-board", "houten plaatmateriaal", "timber board", "Holzwerkstoffplatte", "panneau de bois"),
    ("hout_plaatmateriaal", "vuren", ""): ("spruce-board", "vuren plaat", "spruce board", "Fichtenplatte", "panneau d'épicéa"),
    ("keramiek", "", ""): ("ceramic", "keramiek", "ceramic", "Keramik", "céramique"),
    ("metselwerk", "", ""): ("brick", "metselwerk", "brickwork", "Mauerwerk", "maçonnerie"),
    ("metselwerk", "baksteen", ""): ("clay-brick", "metselwerk van baksteen", "clay brickwork", "Ziegelmauerwerk", "maçonnerie de briques"),
    ("metselwerk", "waalformaat", ""): ("brick-waal-format", "metselwerk, waalformaat", "brickwork, Waal format", "Mauerwerk, Waalformat", "maçonnerie, format Waal"),
    ("kalkzandsteen", "", ""): ("sand-lime", "kalkzandsteen", "sand-lime brick", "Kalksandstein", "brique silico-calcaire"),
    ("riet", "", ""): ("reed", "riet", "reed", "Reet", "chaume"),
    ("gips", "", ""): ("gypsum", "gips", "gypsum", "Gips", "plâtre"),
    ("fermacell", "", ""): ("gypsum-fibre-board", "gipsvezelplaat", "gypsum fibre board", "Gipsfaserplatte", "plaque de plâtre fibrée"),
    ("zandcement", "", ""): ("sand-cement", "zandcement", "sand-cement screed", "Zementestrich", "chape ciment"),
    ("mortel", "", ""): ("mortar", "mortel", "mortar", "Mörtel", "mortier"),
    ("mortel", "cement", ""): ("cement-mortar", "cementmortel", "cement mortar", "Zementmörtel", "mortier de ciment"),
    ("mortel", "tras", ""): ("trass-mortar", "trasmortel", "trass mortar", "Trassmörtel", "mortier de trass"),
    ("mortel", "kalk", ""): ("lime-mortar", "kalkmortel", "lime mortar", "Kalkmörtel", "mortier de chaux"),
    ("anhydriet", "", ""): ("anhydrite", "anhydriet", "anhydrite", "Anhydrit", "anhydrite"),
    ("cellenbeton", "", ""): ("aerated-concrete", "cellenbeton", "aerated concrete", "Porenbeton", "béton cellulaire"),
    ("isolatie", "", ""): ("insulation", "isolatie", "insulation", "Dämmung", "isolant"),
    ("natuursteen", "", ""): ("natural-stone", "natuursteen", "natural stone", "Naturstein", "pierre naturelle"),
    ("EPDM", "", ""): ("epdm", "EPDM", "EPDM", "EPDM", "EPDM"),
    ("bitumen", "", ""): ("bitumen", "bitumen", "bitumen", "Bitumen", "bitume"),
    ("gras", "", ""): ("grass", "gras", "grass", "Gras", "herbe"),
    ("glas", "", ""): ("glass", "glas", "glass", "Glas", "verre"),
    ("generiek", "lichte scheidingswand", ""): ("light-partition", "lichte scheidingswand", "light partition", "leichte Trennwand", "cloison légère"),
}
# a material of the catalogue's kinds that the starter library has under another key
STARTER = {"openaec-17-pir": "insulation-pir", "openaec-17-pur": "insulation-pur", "openaec-17-eps": "insulation-eps", "openaec-17-glass-wool": "glass-wool", "openaec-16-osb": "osb", "openaec-16-plywood": "plywood", "openaec-1-porous-brick": "brick-porous", "openaec-12-spruce-clt": "timber-clt"}
KIND = {1: "brick", 3: "sand-lime", 5: "concrete", 6: "concrete", 7: "concrete", 8: "concrete", 12: "timber", 13: "timber", 14: "timber", 16: "timber", 17: "insulation", 18: "steel"}


def slug(text):
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")


def sign_of(pattern):
    m = re.match(r"NEN47_(\d+)_", pattern)
    return int(m[1]) if m else None


def hatch_named(nl):
    """The suite's hatch of one kind of material called `nl`, else its sign called so (sand, water)."""
    own = [h for h in hatches if h["set"] == "openaec" and h["names"]["nl"] == nl]
    return next((h for h in own if h["key"].count("-") > 1), next(iter(own), None))


def general(sign):
    return next((h["key"] for h in hatches if h["set"] == "openaec" and h["sign"] == sign and h["key"] == f"openaec-{sign}"), None)


rows = xlsx.read(source)["materials"][1:]
out, seen, notes = [], set(), []
for r in rows:
    base, kind, grade = r.get("B", ""), r.get("C", ""), r.get("D", "")
    colour = [int(x) for x in re.findall(r"\d+", r["P"])[:3]]
    sign = sign_of(r.get("F", ""))
    entry = None
    if kind:
        hatch = hatch_named(ALIAS.get(kind, kind.replace("_", " ")))
        if hatch and (base, kind, "") not in NAMED:
            key = STARTER.get(hatch["key"], slug(hatch["names"]["en"]))
            names = dict(hatch["names"])
            if grade:
                key, names = f"{key}-{slug(grade)}", {lang: f"{name} {grade}" for lang, name in names.items()}
            entry = {"key": key, "names": names, "hatch": KIND.get(hatch["sign"], "generic"), "colour": colour, "sign": hatch["key"]}
            if grade:
                entry["grade"] = grade
    if entry is None:
        named = NAMED.get((base, kind, ""))
        if named is None:
            notes.append(f"not named: {base}|{kind}|{grade}")
            continue
        key, *names = named
        if grade:
            key, names = f"{key}-{slug(grade)}", [f"{n} {grade}" for n in names]
        kind_of = "gypsum" if base in ("gips", "fermacell") else KIND.get(sign, "generic")
        entry = {"key": key, "names": dict(zip(["nl", "en", "de", "fr"], names)), "hatch": kind_of, "colour": colour}
        if sign and general(sign):
            entry["sign"] = general(sign)
        if grade:
            entry["grade"] = grade
    wikidata = re.search(r"Q\d+", r.get("L", ""))
    if wikidata:
        entry["wikidata"] = wikidata[0]
    if entry["key"] in seen:
        notes.append(f"twice in the sheet: {entry['key']}")
        continue
    seen.add(entry["key"])
    out.append(entry)

data = {
    "source": "Project Ocondat, OpenAEC Foundation: BlenderBIM/Library.xlsx, sheet materials (https://github.com/OpenAEC-Foundation/project-ocondat)",
    "commit": commit,
    "licence": "LGPL-3.0",
    "materials": out,
}
target = root / "crates/bim-library/data/ocondat-materials.json"
head = "".join(f'  "{k}": {json.dumps(data[k])},\n' for k in ["source", "commit", "licence"])
target.write_text("{\n" + head + '  "materials": [\n' + ",\n".join("    " + json.dumps(m, ensure_ascii=False) for m in out) + "\n  ]\n}\n")
print(len(out), "materials;", sum("sign" in m for m in out), "with a hatch of the catalogue")
print("\n".join(notes))
