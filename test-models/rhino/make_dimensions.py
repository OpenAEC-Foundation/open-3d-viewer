# -*- coding: utf-8 -*-
"""Dimensions and leaders in Rhino 9 (dimensions.3dm), with what Rhino draws of each:
the exploded lines and texts (dimensions.json) and a picture from above."""
import os
import json
import math
import System
import Rhino
import Rhino.Geometry as rg
import scriptcontext as sc

OUT = os.environ.get("MODELS_OUT")


def asc(t):
    return "".join(c if ord(c) < 128 else "<U+%04X>" % ord(c) for c in System.String(t))


def main():
    doc = sc.doc
    for i in [o.Id for o in doc.Objects]:
        doc.Objects.Delete(i, True)

    def layer(name, rgb):
        l = Rhino.DocObjects.Layer()
        l.Name = name
        l.Color = System.Drawing.Color.FromArgb(*rgb)
        return doc.Layers.Add(l)

    def attrs(lay, name):
        a = Rhino.DocObjects.ObjectAttributes()
        a.LayerIndex = lay
        a.Name = name
        return a

    LD = layer("Dimensions", (200, 30, 30))
    LL = layer("Leaders", (30, 120, 30))

    # the current style: model space scale 1, text 1 high, arrows 1 long
    ds = doc.DimStyles.Current
    ds.DimensionScale = 1.0
    ds.TextHeight = 1.0
    ds.ArrowLength = 1.0
    ds.LeaderArrowLength = 1.0
    ds.TextGap = 0.25
    ds.ExtensionLineExtension = 0.5
    ds.ExtensionLineOffset = 0.5
    ds.LengthResolution = 1
    doc.DimStyles.Modify(ds, ds.Id, True)
    style = doc.DimStyles.Current

    # a second style with tick arrows, text in the dimension line, a prefix and a suffix
    ticks = Rhino.DocObjects.DimensionStyle()
    ticks.CopyFrom(style)
    ticks.Name = "Ticks"
    ticks.ArrowType1 = Rhino.DocObjects.DimensionStyle.ArrowType.Tick
    ticks.ArrowType2 = Rhino.DocObjects.DimensionStyle.ArrowType.Tick
    ticks.DimTextLocation = Rhino.DocObjects.DimensionStyle.TextLocation.InDimLine
    ticks.Prefix = "L="
    ticks.Suffix = " mm"
    ticks.LengthResolution = 0
    ti = doc.DimStyles.Add(ticks, False)
    tick_style = doc.DimStyles[ti]

    plane = rg.Plane.WorldXY
    added = []

    def add(geom, name, lay=LD):
        added.append(doc.Objects.Add(geom, attrs(lay, name)))

    # linear: horizontal, vertical, aligned
    add(rg.LinearDimension.Create(Rhino.Geometry.AnnotationType.Rotated, style, plane, rg.Vector3d.XAxis, rg.Point3d(0, 0, 0), rg.Point3d(10, 0, 0), rg.Point3d(5, 3, 0), 0.0), "Horizontal")
    add(rg.LinearDimension.Create(Rhino.Geometry.AnnotationType.Rotated, style, plane, rg.Vector3d.XAxis, rg.Point3d(14, 0, 0), rg.Point3d(14, 8, 0), rg.Point3d(17, 4, 0), math.radians(90)), "Vertical")
    add(rg.LinearDimension.Create(Rhino.Geometry.AnnotationType.Aligned, style, plane, rg.Vector3d.XAxis, rg.Point3d(20, 0, 0), rg.Point3d(26, 8, 0), rg.Point3d(21, 6, 0), 0.0), "Aligned")
    add(rg.LinearDimension.Create(Rhino.Geometry.AnnotationType.Rotated, tick_style, plane, rg.Vector3d.XAxis, rg.Point3d(0, -10, 0), rg.Point3d(12.5, -10, 0), rg.Point3d(6, -7, 0), 0.0), "Ticks in line")

    # angular, radial, diameter
    arc = rg.Arc(rg.Point3d(35, 0, 0), rg.Point3d(41, 0, 0), rg.Point3d(35 + 6 * math.cos(1.0), 6 * math.sin(1.0), 0))
    ang = rg.AngularDimension.Create(style, plane, rg.Vector3d.XAxis, rg.Point3d(35, 0, 0), rg.Point3d(41, 0, 0), rg.Point3d(35 + 6 * math.cos(1.0), 6 * math.sin(1.0), 0), rg.Point3d(35 + 5 * math.cos(0.5), 5 * math.sin(0.5), 0))
    add(ang, "Angle")
    circle = rg.Circle(rg.Point3d(50, 0, 0), 4)
    doc.Objects.AddCircle(circle, attrs(LD, "Circle"))
    add(rg.RadialDimension.Create(style, Rhino.Geometry.AnnotationType.Radius, plane, rg.Point3d(50, 0, 0), rg.Point3d(50 + 4 * math.cos(0.8), 4 * math.sin(0.8), 0), rg.Point3d(57, 6, 0)), "Radius")
    circle2 = rg.Circle(rg.Point3d(62, 0, 0), 3)
    doc.Objects.AddCircle(circle2, attrs(LD, "Circle 2"))
    add(rg.RadialDimension.Create(style, Rhino.Geometry.AnnotationType.Diameter, plane, rg.Point3d(62, 0, 0), rg.Point3d(62 + 3 * math.cos(2.5), 3 * math.sin(2.5), 0), rg.Point3d(56, 5, 0)), "Diameter")

    # ordinates along x and y
    add(rg.OrdinateDimension.Create(style, plane, rg.OrdinateDimension.MeasuredDirection.Xaxis, rg.Point3d(0, 0, 0), rg.Point3d(4, -20, 0), rg.Point3d(4, -25, 0), 0.0, 0.0), "Ordinate x")
    add(rg.OrdinateDimension.Create(style, plane, rg.OrdinateDimension.MeasuredDirection.Yaxis, rg.Point3d(0, 0, 0), rg.Point3d(10, -18, 0), rg.Point3d(15, -18, 0), 0.0, 0.0), "Ordinate y")

    # leaders: a straight one and one with a bend, with text
    add(rg.Leader.Create("Leader", plane, style, System.Array[rg.Point3d]([rg.Point3d(30, -15, 0), rg.Point3d(34, -10, 0)])), "Leader", LL)
    add(rg.Leader.Create("Two\nlines", plane, style, System.Array[rg.Point3d]([rg.Point3d(40, -18, 0), rg.Point3d(42, -12, 0), rg.Point3d(46, -12, 0)])), "Leader bent", LL)

    opts = Rhino.FileIO.FileWriteOptions()
    doc.WriteFile(os.path.join(OUT, "dimensions.3dm"), opts)

    # what Rhino draws of each: exploded lines (sampled) and texts
    ref = {}
    for oid in added:
        o = doc.Objects.FindId(oid)
        g = o.Geometry
        parts = g.Explode() if hasattr(g, "Explode") else []
        lines, texts = [], []
        for p in parts:
            if isinstance(p, rg.Curve):
                pl = p.ToPolyline(0.01, 0.1, 0.0, 0.0) if not isinstance(p, rg.LineCurve) else None
                pts = [p.PointAtStart, p.PointAtEnd] if pl is None else [q for q in pl.ToPolyline()]
                lines.append([[round(q.X, 4), round(q.Y, 4)] for q in pts])
            elif isinstance(p, rg.TextEntity):
                bb = rg.BoundingBox.Empty
                for c in p.Explode():
                    bb.Union(c.GetBoundingBox(True))
                texts.append({"text": asc(p.PlainText), "box": [round(bb.Min.X, 4), round(bb.Min.Y, 4), round(bb.Max.X, 4), round(bb.Max.Y, 4)]})
            elif isinstance(p, rg.Hatch):
                bb = p.GetBoundingBox(True)
                lines.append({"hatch": [round(bb.Min.X, 4), round(bb.Min.Y, 4), round(bb.Max.X, 4), round(bb.Max.Y, 4)]})
            else:
                lines.append({"other": str(type(p))})
        bb = g.GetBoundingBox(True)
        text = ""
        try:
            text = g.PlainText
        except Exception:
            pass
        ref[o.Attributes.Name] = {"class": type(g).__name__, "text": asc(text), "box": [bb.Min.X, bb.Min.Y, bb.Max.X, bb.Max.Y], "lines": lines, "texts": texts}
    with open(os.path.join(OUT, "dimensions.json"), "w") as f:
        f.write(json.dumps(ref, indent=1))

    view = doc.Views.ActiveView
    if view:
        vp = view.ActiveViewport
        vp.ChangeToParallelProjection(True)
        vp.SetCameraLocation(rg.Point3d(30, -5, 100), False)
        vp.SetCameraDirection(-rg.Vector3d.ZAxis, False)
        vp.CameraUp = rg.Vector3d.YAxis
        vp.ZoomExtents()
        view.Redraw()
        DMD = Rhino.Display.DisplayModeDescription
        mode = DMD.GetDisplayMode(DMD.WireframeId)
        bmp = view.CaptureToBitmap(System.Drawing.Size(1800, 900), mode)
        if bmp:
            bmp.Save(os.path.join(OUT, "dimensions.png"))
    doc.Modified = False


try:
    main()
except Exception:
    import traceback
    with open(os.path.join(OUT, "dimensions_error.txt"), "w") as f:
        f.write(traceback.format_exc())
