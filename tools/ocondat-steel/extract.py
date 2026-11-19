#!/usr/bin/env python3
"""Extracts the steel profiles of Project Ocondat for the steel table of `bim-library`.

    python3 tools/ocondat-steel/extract.py PATH/TO/project-ocondat/steelprofile.json COMMIT

writes `crates/bim-library/data/ocondat-steel.json`: per series the rows
`[designation, h, b, tw, tf, r1, r2]` in millimetres, as `profile.rs` reads them.

Taken over are the series the suite's own table lacks (HEAA, HD, UAP, cold-formed hollow
sections, flat and round bars, the old Differdinger beams) and every size of angles, tees and
hot-finished hollow sections; `profile.rs` skips a row whose designation its own table has.
Left out: IPE, INP, HEA, HEB, HEM, UNP and UPE (the suite's table has them from the standards)
and the old sloped series "B" and "N.P.", whose flange slope the source does not give. A row
that is no shape is left out and printed (an angle without a flat toe, a tube without a wall).

Corner radii of hollow sections are not taken from the source, where they are inconsistent,
but from the product standards: `profile.rs` sets them from the wall thickness.
"""
import json
import re
import sys
from pathlib import Path

source, commit = sys.argv[1], sys.argv[2]
rows = {k: v[0] for e in json.load(open(source)) for k, v in e.items()}


def n(x):
    """A number as the table writes it: whole where it is whole."""
    x = round(float(x), 3)
    return int(x) if x == int(x) else x


def dims(*xs):
    return "x".join(str(n(x)) for x in xs)


series = {k: [] for k in ["heaa", "hd", "uap", "l-equal", "l-unequal", "t", "shs", "rhs", "cf-shs", "cf-rhs", "chs", "flat", "round", "differdinger"]}
skipped = []
for key, row in rows.items():
    c, shape = row["shape_coords"], row["shape_name"]
    if shape == "I-shape parallel flange":
        h, b, tw, tf, r = c[:5]
        if m := re.fullmatch(r"(\d+)AA", key):
            series["heaa"].append([f"HEAA {m[1]}", h, b, tw, tf, r, 0])
        elif m := re.fullmatch(r"HD(\d+)/([\d,]+)", key):
            series["hd"].append([f"HD {m[1]}x{m[2].replace(',', '.')}", h, b, tw, tf, r, 0])
        elif m := re.fullmatch(r"(DI[ELNR])(\d+(?:\.\d+)?)", key):
            series["differdinger"].append([f"{m[1]} {m[2]}", h, b, tw, tf, r, 0])
        elif not re.match(r"(HEA|HEB|HEM|IPE)\d+$", key):
            skipped.append(key)
    elif shape == "C-channel parallel flange":
        h, b, tw, tf, r = c[:5]
        if m := re.fullmatch(r"UAP(\d+)", key):
            series["uap"].append([f"UAP {m[1]}", h, b, tw, tf, r, 0])
        elif not key.startswith("UPE"):
            skipped.append(key)
    elif shape == "LAngle":
        h, b, t, _, r1, r2 = c[:6]
        # a toe radius is half the root radius; "L90/60/6" has 47.5 for 4.75
        r2 = min(r2, r1 / 2)
        if r2 >= t:
            # no flat left on the toe ("L40/40/3"): not a shape the suite draws
            skipped.append(key)
            continue
        series["l-equal" if h == b else "l-unequal"].append([f"L {dims(h, b, t)}", h, b, t, t, r1, r2])
    elif shape == "TProfile":
        h, b, tw, tf, r = c[:5]
        series["t"].append([f"T {n(h)}" if h == b else f"T {dims(h, b, tw)}", h, b, tw, tf, r, 0])
    elif shape == "Rectangle Hollow Section":
        h, b, t = c[:3]
        cold = key.startswith("CFRHS")
        # "CFRHS120X120X" has no wall
        if t <= 0 or not cold and not key.startswith("K"):
            skipped.append(key)
            continue
        name = ("CF" if cold else "") + ("SHS" if h == b else "RHS")
        series[("cf-" if cold else "") + ("shs" if h == b else "rhs")].append([f"{name} {dims(h, b, t)}", h, b, t, 0, 0, 0])
    elif shape == "Round tube profile":
        d, t = c[:2]
        series["chs"].append([f"CHS {dims(d, t)}", d, d, t, 0, 0, 0])
    elif shape == "Rectangle":
        b, t = c[:2]
        series["flat"].append([f"FL {dims(b, t)}", t, b, t, 0, 0, 0])
    elif shape == "Round":
        d = c[0]
        series["round"].append([f"RD {n(d)}", d, d, 0, 0, 0, 0])
    elif not re.match(r"(INP|UNP)\d+$", key) and not re.search(r" (B|N\.P\.)$", key):
        skipped.append(key)

for name, of in series.items():
    seen = set()
    of[:] = [[r[0]] + [n(x) for x in r[1:]] for r in of if not (r[0] in seen or seen.add(r[0]))]
    of.sort(key=lambda r: (r[1], r[2], r[3], r[0]))

out = {
    "source": "Project Ocondat, OpenAEC Foundation: steelprofile.json (https://github.com/OpenAEC-Foundation/project-ocondat)",
    "commit": commit,
    "licence": "LGPL-3.0",
    "rows": "designation, h, b, tw, tf, r1, r2 in millimetres",
    "series": series,
}
target = Path(__file__).resolve().parents[2] / "crates/bim-library/data/ocondat-steel.json"
lines = ",\n".join(f'    "{k}": [\n' + ",\n".join("      " + json.dumps(r) for r in v) + "\n    ]" for k, v in series.items())
head = "".join(f'  "{k}": {json.dumps(out[k])},\n' for k in ["source", "commit", "licence", "rows"])
target.write_text("{\n" + head + '  "series": {\n' + lines + "\n  }\n}\n")
print({k: len(v) for k, v in series.items()}, "skipped:", skipped)
