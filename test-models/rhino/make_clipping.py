# -*- coding: utf-8 -*-
"""Clipping planes (clipping.3dm): a box, a plane at 5 mm that cuts away its top in the
Perspective viewport, one that clips the Top viewport only to a depth of 2 mm, and a disabled
one; with what Rhino reads of them (clipping.json)."""
import os
import json
import Rhino
import Rhino.Geometry as rg
import scriptcontext as sc

OUT = os.environ.get("MODELS_OUT")


def viewport(doc, name):
    for v in doc.Views:
        if v.ActiveViewport.Name == name:
            return v.ActiveViewport.Id
    raise Exception("no viewport " + name)


def main():
    doc = sc.doc
    for i in [o.Id for o in doc.Objects]:
        doc.Objects.Delete(i, True)
    box = rg.Box(rg.Plane.WorldXY, rg.Interval(0, 10), rg.Interval(0, 10), rg.Interval(0, 10))
    doc.Objects.AddBrep(box.ToBrep())
    perspective = viewport(doc, "Perspective")
    top = viewport(doc, "Top")
    # normal up: cuts away what lies above 5 mm
    cut = doc.Objects.AddClippingPlane(rg.Plane(rg.Point3d(-2, -2, 5), rg.Vector3d.XAxis, rg.Vector3d.YAxis), 14, 14, perspective)
    # normal down through 8 mm, clipping the Top viewport to a depth of 2 mm
    slab = doc.Objects.AddClippingPlane(rg.Plane(rg.Point3d(-2, 12, 8), rg.Vector3d.XAxis, -rg.Vector3d.YAxis), 14, 14, top)
    ob = doc.Objects.FindId(slab)
    ob.ClippingPlaneGeometry.PlaneDepth = 2.0 if hasattr(ob, "ClippingPlaneGeometry") else None
    g = ob.ClippingPlaneGeometry if hasattr(ob, "ClippingPlaneGeometry") else ob.Geometry
    g.PlaneDepth = 2.0
    g.PlaneDepthEnabled = True
    ob.CommitChanges()
    # an x plane, switched off
    off = doc.Objects.AddClippingPlane(rg.Plane(rg.Point3d(3, -2, -2), rg.Vector3d.YAxis, rg.Vector3d.ZAxis), 14, 14, perspective)
    doc.Objects.UnselectAll()
    doc.Objects.Select(off)
    Rhino.RhinoApp.RunScript("_DisableClippingPlane", False)
    doc.Objects.UnselectAll()
    for i, nm in [(cut, "Cut"), (slab, "Slab"), (off, "Off")]:
        ob = doc.Objects.FindId(i)
        a = ob.Attributes.Duplicate()
        a.Name = nm
        doc.Objects.ModifyAttributes(i, a, True)
    doc.WriteFile(os.path.join(OUT, "clipping.3dm"), Rhino.FileIO.FileWriteOptions())
    # what Rhino reads back
    f = Rhino.FileIO.File3dm.Read(os.path.join(OUT, "clipping.3dm"))
    out = {"views": [{"name": v.Name, "id": str(v.Viewport.Id), "perspective": v.Viewport.IsPerspectiveProjection} for v in f.Views], "planes": []}
    for ob in f.Objects:
        g = ob.Geometry
        if isinstance(g, rg.ClippingPlaneSurface):
            p = g.Plane
            out["planes"].append({
                "name": ob.Attributes.Name,
                "origin": [p.Origin.X, p.Origin.Y, p.Origin.Z],
                "normal": [p.ZAxis.X, p.ZAxis.Y, p.ZAxis.Z],
                "corners": [[c.X, c.Y, c.Z] for c in [g.PointAt(g.Domain(0).Min, g.Domain(1).Min), g.PointAt(g.Domain(0).Max, g.Domain(1).Min), g.PointAt(g.Domain(0).Max, g.Domain(1).Max), g.PointAt(g.Domain(0).Min, g.Domain(1).Max)]],
                "viewports": [str(i) for i in g.ViewportIds()],
                "depth": g.PlaneDepth if g.PlaneDepthEnabled else None,
            })
    with open(os.path.join(OUT, "clipping.json"), "w") as fh:
        json.dump(out, fh, indent=1)


try:
    main()
except Exception:
    import traceback
    with open(os.path.join(OUT, "clipping_error.txt"), "w") as fh:
        fh.write(traceback.format_exc())
