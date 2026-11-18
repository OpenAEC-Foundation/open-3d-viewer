# -*- coding: utf-8 -*-
"""Object attributes (attributes.3dm): curves in groups, with a url, print colour and width, a
linetype scale and an object's own linetype; a hatch with a background fill and its boundary;
an object shown in the Top viewport only; draw order. What Rhino reads back: attributes.json."""
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
    g1 = doc.Groups.Add("Fence")
    g2 = doc.Groups.Add("North side")
    dashed = doc.Linetypes.Find("Dashed")
    if dashed < 0:
        doc.Linetypes.LoadDefaultLinetypes()
        dashed = doc.Linetypes.Find("Dashed")
    a = Rhino.DocObjects.ObjectAttributes()
    a.Name = "Grouped"
    a.AddToGroup(g1)
    a.AddToGroup(g2)
    a.Url = "https://example.org/fence"
    a.PlotColorSource = Rhino.DocObjects.ObjectPlotColorSource.PlotColorFromObject
    a.PlotColor = System.Drawing.Color.FromArgb(10, 20, 30)
    a.PlotWeightSource = Rhino.DocObjects.ObjectPlotWeightSource.PlotWeightFromObject
    a.PlotWeight = 0.35
    a.LinetypeIndex = dashed
    a.LinetypeSource = Rhino.DocObjects.ObjectLinetypeSource.LinetypeFromObject
    a.LinetypePatternScale = 2.0
    a.DisplayOrder = 3
    doc.Objects.AddLine(rg.Line(rg.Point3d(0, 0, 0), rg.Point3d(100, 0, 0)), a)
    # its own linetype: 4 dash, 1 gap
    b = Rhino.DocObjects.ObjectAttributes()
    b.Name = "Own linetype"
    lt = Rhino.DocObjects.Linetype()
    lt.AppendSegment(4.0, True)
    lt.AppendSegment(1.0, False)
    try:
        b.SetCustomLinetype(lt)
    except Exception:
        pass
    doc.Objects.AddLine(rg.Line(rg.Point3d(0, -10, 0), rg.Point3d(100, -10, 0)), b)
    # a hatch with a background fill, its boundary drawn
    c = Rhino.DocObjects.ObjectAttributes()
    c.Name = "Filled hatch"
    try:
        c.HatchBackgroundFillColor = System.Drawing.Color.FromArgb(200, 220, 255)
        c.HatchBoundaryVisible = True
    except Exception:
        pass
    square = rg.Polyline([rg.Point3d(0, -40, 0), rg.Point3d(20, -40, 0), rg.Point3d(20, -20, 0), rg.Point3d(0, -20, 0), rg.Point3d(0, -40, 0)]).ToNurbsCurve()
    pattern = doc.HatchPatterns.FindName("Hatch1")
    hatches = rg.Hatch.Create(square, pattern.Index if pattern else 0, 0, 1, 0.001)
    doc.Objects.AddHatch(hatches[0], c)
    # shown in the Top viewport only
    top = [v for v in doc.Views if v.ActiveViewport.Name == "Top"][0].ActiveViewport.Id
    d = Rhino.DocObjects.ObjectAttributes()
    d.Name = "Top only"
    d.ViewportId = top
    doc.Objects.AddLine(rg.Line(rg.Point3d(0, -50, 0), rg.Point3d(100, -50, 0)), d)
    doc.WriteFile(os.path.join(OUT, "attributes.3dm"), Rhino.FileIO.FileWriteOptions())
    f = Rhino.FileIO.File3dm.Read(os.path.join(OUT, "attributes.3dm"))
    out = {"groups": [{"index": g.Index, "name": g.Name} for g in f.AllGroups], "objects": []}
    for o in f.Objects:
        at = o.Attributes
        own = None
        try:
            cl = at.GetCustomLinetype()
            if cl:
                own = []
                for k in range(cl.SegmentCount):
                    length, solid = cl.GetSegment(k)
                    own.append(length if solid else -length)
        except Exception:
            pass
        bg = None
        bv = None
        try:
            col = at.HatchBackgroundFillColor
            bg = [int(col.R), int(col.G), int(col.B), 255 - int(col.A)] if int(col.A) != 0 else None
            bv = at.HatchBoundaryVisible
        except Exception:
            pass
        out["objects"].append({
            "name": at.Name,
            "groups": [int(x) for x in (at.GetGroupList() or [])],
            "url": at.Url,
            "plot_color": [int(at.PlotColor.R), int(at.PlotColor.G), int(at.PlotColor.B)],
            "plot_weight": at.PlotWeight,
            "plot_color_source": int(at.PlotColorSource),
            "plot_weight_source": int(at.PlotWeightSource),
            "linetype_scale": at.LinetypePatternScale,
            "display_order": int(at.DisplayOrder),
            "viewport": str(at.ViewportId),
            "custom_linetype": own,
            "hatch_background": bg,
            "hatch_boundary": bv,
        })
    with open(os.path.join(OUT, "attributes.json"), "w") as fh:
        json.dump(out, fh, indent=1)


try:
    main()
except Exception:
    import traceback
    with open(os.path.join(OUT, "attributes_error.txt"), "w") as fh:
        fh.write(traceback.format_exc())
