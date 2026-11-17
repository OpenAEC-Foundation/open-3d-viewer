# -*- coding: utf-8 -*-
"""Curves in Rhino's linetypes (linetypes.3dm): by object, by layer, a circle, with Rhino's
linetype data and display scale (linetypes.txt) and a picture from above."""
import os
import System
import Rhino
import Rhino.Geometry as rg
import scriptcontext as sc

OUT = os.environ.get("MODELS_OUT")


def main():
    doc = sc.doc
    for i in [o.Id for o in doc.Objects]:
        doc.Objects.Delete(i, True)
    lines = []
    names = ["Continuous", "Dashed", "Dots", "DashDot", "Hidden", "Center", "Border"]
    y = 0.0
    for nm in names:
        idx = doc.Linetypes.Find(nm)
        if idx < 0:
            idx = doc.Linetypes.LoadDefaultLinetypes() and doc.Linetypes.Find(nm)
        a = Rhino.DocObjects.ObjectAttributes()
        a.Name = nm
        if idx >= 0:
            a.LinetypeIndex = idx
            a.LinetypeSource = Rhino.DocObjects.ObjectLinetypeSource.LinetypeFromObject
        doc.Objects.AddLine(rg.Line(rg.Point3d(0, y, 0), rg.Point3d(100, y, 0)), a)
        y -= 5.0
    # a layer whose curves are dashed, and a circle on it
    l = Rhino.DocObjects.Layer()
    l.Name = "Dashed layer"
    l.LinetypeIndex = doc.Linetypes.Find("Dashed")
    li = doc.Layers.Add(l)
    a = Rhino.DocObjects.ObjectAttributes()
    a.LayerIndex = li
    a.Name = "Circle by layer"
    doc.Objects.AddCircle(rg.Circle(rg.Point3d(120, -15, 0), 15), a)
    doc.WriteFile(os.path.join(OUT, "linetypes.3dm"), Rhino.FileIO.FileWriteOptions())
    for lt in doc.Linetypes:
        segs = []
        for k in range(lt.SegmentCount):
            length, solid = lt.GetSegment(k)
            segs.append("%g%s" % (length, "" if solid else "_"))
        lines.append("%s %d: %s pattern %g" % (lt.Name, lt.Index, " ".join(segs), lt.PatternLength))
    for prop in ["LinetypeDisplayScale", "ModelSpaceLinetypeScale"]:
        try:
            lines.append("%s %s" % (prop, getattr(doc, prop)))
        except Exception as e:
            lines.append("%s ? %s" % (prop, e))
    try:
        lines.append("ModelUnits %s" % doc.ModelUnitSystem)
        lines.append("Linetype display: %s" % Rhino.ApplicationSettings.ViewSettings.LinetypeDisplayScale if hasattr(Rhino.ApplicationSettings.ViewSettings, "LinetypeDisplayScale") else "")
    except Exception as e:
        lines.append(str(e))
    with open(os.path.join(OUT, "linetypes.txt"), "w") as f:
        f.write("\n".join(lines))
    view = doc.Views.ActiveView
    if view:
        vp = view.ActiveViewport
        vp.ChangeToParallelProjection(True)
        vp.SetCameraLocation(rg.Point3d(70, -15, 100), False)
        vp.SetCameraDirection(-rg.Vector3d.ZAxis, False)
        vp.CameraUp = rg.Vector3d.YAxis
        vp.ZoomExtents()
        view.Redraw()
        DMD = Rhino.Display.DisplayModeDescription
        bmp = view.CaptureToBitmap(System.Drawing.Size(1800, 700), DMD.GetDisplayMode(DMD.WireframeId))
        if bmp:
            bmp.Save(os.path.join(OUT, "linetypes.png"))
    doc.Modified = False


try:
    main()
except Exception:
    import traceback
    with open(os.path.join(OUT, "linetypes_error.txt"), "w") as f:
        f.write(traceback.format_exc())
