# -*- coding: utf-8 -*-
"""Text dots (dots.3dm): a box with two dots on its corners, one red by object on the default
layer and one on a layer that is off; dots.json with what Rhino reads back."""
import os
import json
import System
import Rhino
import Rhino.Geometry as rg
import scriptcontext as sc

OUT = os.environ.get("MODELS_OUT")


def main():
    doc = sc.doc
    for i in [o.Id for o in doc.Objects]:
        doc.Objects.Delete(i, True)
    doc.AdjustModelUnitSystem(Rhino.UnitSystem.Millimeters, False)
    box = rg.Box(rg.Plane.WorldXY, rg.Interval(0, 1000), rg.Interval(0, 600), rg.Interval(0, 300))
    doc.Objects.AddBrep(box.ToBrep())
    a = Rhino.DocObjects.ObjectAttributes()
    a.ColorSource = Rhino.DocObjects.ObjectColorSource.ColorFromObject
    a.ObjectColor = System.Drawing.Color.FromArgb(200, 30, 30)
    doc.Objects.AddTextDot(rg.TextDot("A1", rg.Point3d(1000, 600, 300)), a)
    l = Rhino.DocObjects.Layer()
    l.Name = "Notes"
    l.IsVisible = False
    li = doc.Layers.Add(l)
    b = Rhino.DocObjects.ObjectAttributes()
    b.LayerIndex = li
    doc.Objects.AddTextDot(rg.TextDot("Hidden note", rg.Point3d(0, 0, 0)), b)
    doc.WriteFile(os.path.join(OUT, "dots.3dm"), Rhino.FileIO.FileWriteOptions())
    f = Rhino.FileIO.File3dm.Read(os.path.join(OUT, "dots.3dm"))
    out = []
    for o in f.Objects:
        g = o.Geometry
        if isinstance(g, rg.TextDot):
            out.append({"text": g.Text, "point": [g.Point.X, g.Point.Y, g.Point.Z], "layer": f.AllLayers.FindIndex(o.Attributes.LayerIndex).Name})
    with open(os.path.join(OUT, "dots.json"), "w") as fh:
        json.dump(out, fh, indent=1)


try:
    main()
except Exception:
    import traceback
    with open(os.path.join(OUT, "dots_error.txt"), "w") as fh:
        fh.write(traceback.format_exc())
