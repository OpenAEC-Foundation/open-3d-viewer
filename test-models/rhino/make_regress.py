# -*- coding: utf-8 -*-
"""Shapes that broke earlier meshing (regress.3dm, no render meshes) and their areas in Rhino
(regress.json): a short conical band of a rational circle, strips of arcs that meet at kinks,
and meshes with strongly bent quads."""
import os
import json
import math
import System
import Rhino
import Rhino.Geometry as rg
import scriptcontext as sc

OUT = os.environ.get("MODELS_OUT")
for i in [o.Id for o in sc.doc.Objects]:
    sc.doc.Objects.Delete(i, True)
lay = sc.doc.Layers.Add("Regressions", System.Drawing.Color.FromArgb(120, 150, 90))


def add(geom, name):
    a = Rhino.DocObjects.ObjectAttributes()
    a.LayerIndex = lay
    a.Name = name
    if isinstance(geom, rg.Mesh):
        return sc.doc.Objects.AddMesh(geom, a)
    return sc.doc.Objects.AddBrep(geom, a)


# 1. a lens ring: a short cone band, revolved, as a rational NURBS surface of one face
line = rg.LineCurve(rg.Point3d(2.5, 0, 0), rg.Point3d(2.3, 0, 0.5))
band = rg.RevSurface.Create(line, rg.Line(rg.Point3d(0, 0, 0), rg.Point3d(0, 0, 1))).ToNurbsSurface()
add(band.ToBrep(), "Lens band")
thin = rg.RevSurface.Create(rg.LineCurve(rg.Point3d(8, 0, 0), rg.Point3d(8, 0, 0.05)), rg.Line(rg.Point3d(0, 0, 0), rg.Point3d(0, 0, 1))).ToNurbsSurface()
thin.Translate(rg.Vector3d(20, 0, 0))
add(thin.ToBrep(), "Thin ring")

# 2. strips along curves of arcs that meet at kinks, as one NURBS surface each
def arc_polygon(cx, n, r, bulge):
    pc = rg.PolyCurve()
    pts = [rg.Point3d(cx + r * math.cos(2 * math.pi * k / n), r * math.sin(2 * math.pi * k / n), 0) for k in range(n + 1)]
    for k in range(n):
        a, b = pts[k], pts[k + 1]
        mid = 0.5 * (a + b)
        out = rg.Vector3d(mid.X - cx, mid.Y, 0)
        out.Unitize()
        pc.Append(rg.Arc(a, mid + out * bulge, b))
    return pc.ToNurbsCurve()

for name, cx, n, bulge, h in (("Arc strip", 40, 5, 0.6, 0.2), ("Arc band", 50, 6, 0.4, 1.5), ("Twisted arc strip", 60, 4, 0.8, 0.3)):
    c = arc_polygon(cx, n, 3.0, bulge)
    if name.startswith("Twisted"):
        top = c.DuplicateCurve()
        top.Rotate(0.4, rg.Vector3d.ZAxis, rg.Point3d(cx, 0, 0))
        top.Translate(rg.Vector3d(0, 0, h))
        b = rg.Brep.CreateFromLoft([c, top], rg.Point3d.Unset, rg.Point3d.Unset, rg.LoftType.Straight, False)
        add(b[0], name)
    else:
        s = rg.Surface.CreateExtrusion(c, rg.Vector3d(0, 0, h))
        add(s.ToBrep(), name)

# 3. meshes of strongly bent quads
m = rg.Mesh()
for i in range(6):
    for j in range(6):
        m.Vertices.Add(70 + i, j, 0.8 * ((i + j) % 2))
for i in range(5):
    for j in range(5):
        m.Faces.AddFace(i * 6 + j, (i + 1) * 6 + j, (i + 1) * 6 + j + 1, i * 6 + j + 1)
m.Normals.ComputeNormals()
add(m, "Bent quads")

opts = Rhino.FileIO.FileWriteOptions()
opts.IncludeRenderMeshes = False
sc.doc.WriteFile(os.path.join(OUT, "regress.3dm"), opts)
rows = {}
for o in sc.doc.Objects:
    g = o.Geometry
    amp = rg.AreaMassProperties.Compute(g)
    rows[str(o.Attributes.Name)] = float(amp.Area) if amp else None
open(os.path.join(OUT, "regress.json"), "w").write(json.dumps(rows, indent=1))
sc.doc.Modified = False
