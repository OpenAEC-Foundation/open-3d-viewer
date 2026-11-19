#!/usr/bin/env python3
"""Extracts the timber strength classes of Project Ocondat for `bim-library`.

    python3 tools/ocondat-grades/extract.py PATH/TO/project-ocondat/Libraries/Netherlands/data/hout.py COMMIT

writes `crates/bim-library/data/ocondat-timber-grades.json`: the strength classes of softwood
(C14 to C35) and hardwood (D30 to D70) of EN 338 and of homogeneous glued laminated timber
(GL24h to GL36h) of EN 14080, each with its characteristic strengths and its stiffness as the
file has them: strengths and moduli in N/mm², the characteristic density in kN/m³.

The file has 3.0 kN/m³ for C18, between the 3.1 of C16 and the 3.3 of C20; EN 338 has 320
kg/m³, and 3.2 is written.

The file is read as text and its three tables are taken as literals; nothing of it is run.
"""
import ast
import json
import sys
from pathlib import Path

source, commit = sys.argv[1], sys.argv[2]
tables = {"NAALDHOUT": "softwood", "LOOFHOUT": "hardwood", "GELAMINEERD_HOUT": "glulam"}
grades = []
for node in ast.parse(Path(source).read_text(encoding="utf-8")).body:
    if isinstance(node, ast.Assign) and len(node.targets) == 1 and getattr(node.targets[0], "id", None) in tables:
        for grade, values in ast.literal_eval(node.value).items():
            grades.append({"grade": grade, "kind": tables[node.targets[0].id], **{key.lower(): value for key, value in values.items()}})
for grade in grades:
    if grade["grade"] == "C18" and grade["rho_k"] == 3.0:
        grade["rho_k"] = 3.2
data = {
    "source": "Project Ocondat, OpenAEC Foundation: Libraries/Netherlands/data/hout.py (https://github.com/OpenAEC-Foundation/project-ocondat)",
    "commit": commit,
    "licence": "LGPL-3.0",
    "units": "strengths (fm_k, ft_0_k, ft_90_k, fc_0_k, fc_90_k, fv_k) and moduli (e_0_u_k, e_0_mean, e_90_mean, g_mean) in N/mm2; rho_k in kN/m3",
}
target = Path(__file__).resolve().parents[2] / "crates/bim-library/data/ocondat-timber-grades.json"
head = "".join(f'  "{k}": {json.dumps(v)},\n' for k, v in data.items())
target.write_text("{\n" + head + '  "grades": [\n' + ",\n".join("    " + json.dumps(g) for g in grades) + "\n  ]\n}\n")
print(len(grades), "grades:", " ".join(g["grade"] for g in grades))
