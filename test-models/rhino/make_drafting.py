# -*- coding: utf-8 -*-
"""Curves, texts and hatches in Rhino 9 (drafting.3dm) and a picture of it from above."""
import os
import math
import System
import Rhino
import Rhino.Geometry as rg
import scriptcontext as sc

OUT = os.environ.get("MODELS_OUT")


def main():
    for i in [o.Id for o in sc.doc.Objects]:
        sc.doc.Objects.Delete(i, True)


    def layer(name, rgb):
        l = Rhino.DocObjects.Layer()
        l.Name = name
        l.Color = System.Drawing.Color.FromArgb(*rgb)
        return sc.doc.Layers.Add(l)


    def attrs(lay, name):
        a = Rhino.DocObjects.ObjectAttributes()
        a.LayerIndex = lay
        a.Name = name
        return a


    LC = layer("Curves", (30, 90, 200))
    LT = layer("Text", (20, 20, 20))
    LH = layer("Hatches", (200, 80, 40))

    # curves
    sc.doc.Objects.AddLine(rg.Line(rg.Point3d(0, 0, 0), rg.Point3d(10, 0, 0)), attrs(LC, "Line"))
    sc.doc.Objects.AddCircle(rg.Circle(rg.Point3d(15, 3, 0), 3), attrs(LC, "Circle"))
    sc.doc.Objects.AddArc(rg.Arc(rg.Point3d(20, 0, 0), rg.Point3d(23, 3, 0), rg.Point3d(26, 0, 0)), attrs(LC, "Arc"))
    sc.doc.Objects.AddPolyline(rg.Polyline([rg.Point3d(28, 0, 0), rg.Point3d(30, 4, 0), rg.Point3d(32, 0, 0), rg.Point3d(34, 4, 0)]), attrs(LC, "Polyline"))
    sc.doc.Objects.AddCurve(rg.Curve.CreateInterpolatedCurve([rg.Point3d(36 + i, math.sin(i), 0.3 * i) for i in range(8)], 3), attrs(LC, "Spline"))
    pc = rg.PolyCurve()
    pc.Append(rg.Line(rg.Point3d(0, -6, 0), rg.Point3d(4, -6, 0)))
    pc.Append(rg.Arc(rg.Point3d(4, -6, 0), rg.Point3d(6, -4, 0), rg.Point3d(4, -2, 0)))
    pc.Append(rg.Line(rg.Point3d(4, -2, 0), rg.Point3d(0, -2, 0)))
    sc.doc.Objects.AddCurve(pc, attrs(LC, "Polycurve"))
    sc.doc.Objects.AddEllipse(rg.Ellipse(rg.Plane(rg.Point3d(12, -4, 0), rg.Vector3d.ZAxis), 4, 1.5), attrs(LC, "Ellipse"))

    # texts: the style draws them twice their height in model space
    ds = sc.doc.DimStyles.Current
    ds.DimensionScale = 2.0
    sc.doc.DimStyles.Modify(ds, ds.Id, True)


    def text(s, x, y, h, name, justification=None, rotate=0.0):
        pl = rg.Plane(rg.Point3d(x, y, 0), rg.Vector3d.ZAxis)
        if rotate:
            pl.Rotate(rotate, pl.ZAxis)
        j = justification if justification is not None else Rhino.Geometry.TextJustification.BottomLeft
        sc.doc.Objects.AddText(s, pl, h / 2.0, "Arial", False, False, j, attrs(LT, name))

    text("Rhino 9 tekst", 0, -12, 1.0, "Text 1")
    text("Groot", 0, -16, 2.5, "Text 2")
    text("Twee\nregels", 15, -12, 1.0, "Text 3")
    text("Gedraaid 30", 25, -12, 1.0, "Text 4", rotate=math.radians(30))
    text("Midden", 40, -12, 1.0, "Text 5", justification=Rhino.Geometry.TextJustification.MiddleCenter)
    text("café € 3,5 m²", 0, -20, 1.0, "Text 6")

    # hatches: solid, two line patterns, with a hole, rotated and scaled
    D = Rhino.DocObjects.HatchPattern.Defaults
    PATTERNS = {}
    for nm in ("Solid", "Hatch1", "Hatch2", "Grid", "Plus"):
        pat = getattr(D, nm, None)
        if pat is not None:
            PATTERNS[nm] = sc.doc.HatchPatterns.Add(pat)

    def hatch(curves, pattern_name, name, scale=1.0, rotation=0.0):
        index = PATTERNS.get(pattern_name, 0)
        hs = rg.Hatch.Create(curves, index, rotation, scale, sc.doc.ModelAbsoluteTolerance)
        for h in hs:
            sc.doc.Objects.AddHatch(h, attrs(LH, name))

    sq = lambda x, y, s: rg.Rectangle3d(rg.Plane(rg.Point3d(x, y, 0), rg.Vector3d.ZAxis), s, s).ToNurbsCurve()
    hatch([sq(0, -32, 8), rg.Circle(rg.Point3d(4, -28, 0), 2).ToNurbsCurve()], "Solid", "Solid with hole")
    hatch([sq(12, -32, 8)], "Hatch1", "Hatch1")
    hatch([sq(24, -32, 8)], "Grid", "Grid")
    hatch([sq(36, -32, 8)], "Hatch2", "Hatch2 rotated", 2.0, math.radians(30))
    hatch([rg.Circle(rg.Point3d(52, -28, 0), 4).ToNurbsCurve()], "Plus", "Plus")

    opts = Rhino.FileIO.FileWriteOptions()
    sc.doc.WriteFile(os.path.join(OUT, "drafting.3dm"), opts)
    view = sc.doc.Views.ActiveView
    if view:
        vp = view.ActiveViewport
        vp.ChangeToParallelProjection(True)
        vp.SetCameraLocation(rg.Point3d(25, -15, 100), False)
        vp.SetCameraDirection(-rg.Vector3d.ZAxis, False)
        vp.CameraUp = rg.Vector3d.YAxis
        DMD = Rhino.Display.DisplayModeDescription
        mode = DMD.GetDisplayMode(DMD.ShadedId)
        vp.DisplayMode = mode
        vp.ZoomExtents()
        view.Redraw()
        bmp = view.CaptureToBitmap(System.Drawing.Size(1600, 1000), mode)
        if bmp:
            bmp.Save(os.path.join(OUT, "drafting.png"))
    names = [str(p.Name) for p in sc.doc.HatchPatterns]
    lines = ["patterns: " + ", ".join(names)]
    for o in sc.doc.Objects:
        g = o.Geometry
        if isinstance(g, rg.TextEntity):
            bb = rg.BoundingBox.Empty
            for c in g.Explode():
                bb.Union(c.GetBoundingBox(True))
            lines.append("%s|%.4f %.4f %.4f %.4f" % (o.Attributes.Name, bb.Min.X, bb.Min.Y, bb.Max.X, bb.Max.Y))
    open(os.path.join(OUT, "drafting.txt"), "w").write(chr(10).join(lines))
    sc.doc.Modified = False


try:
    main()
except Exception:
    import traceback
    open(os.path.join(OUT, "drafting_error.txt"), "w").write(traceback.format_exc())
