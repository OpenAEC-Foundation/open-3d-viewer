# -*- coding: utf-8 -*-
"""Layouts (layouts.3dm): a box of 10 by 6 by 3 m and two layouts, "Plan" on A3 landscape with a
locked Top detail at 1:100 and "View" on A4 portrait with a Perspective detail, a text and a line
on the first page; with what Rhino reads of them (layouts.json): the pages (name, paper, page
number, viewport id), the details (boundary corners, projection, camera, scale, lock) and the
page of each page object."""
import os
import json
import Rhino
import Rhino.Geometry as rg
import scriptcontext as sc

OUT = os.environ.get("MODELS_OUT")


def xyz(p):
    return [p.X, p.Y, p.Z]


def main():
    doc = sc.doc
    for i in [o.Id for o in doc.Objects]:
        doc.Objects.Delete(i, True)
    doc.ModelUnitSystem = Rhino.UnitSystem.Millimeters
    doc.PageUnitSystem = Rhino.UnitSystem.Millimeters
    box = rg.Box(rg.Plane.WorldXY, rg.Interval(0, 10000), rg.Interval(0, 6000), rg.Interval(0, 3000))
    doc.Objects.AddBrep(box.ToBrep())

    plan = doc.Views.AddPageView("Plan", 420, 297)
    top = plan.AddDetailView("Top detail", rg.Point2d(20, 20), rg.Point2d(220, 200), Rhino.Display.DefinedViewportProjection.Top)
    plan.SetActiveDetail(top.Id)
    top.Viewport.SetCameraTarget(rg.Point3d(5000, 3000, 0), True)
    top.DetailGeometry.SetScale(100, doc.ModelUnitSystem, 1, doc.PageUnitSystem)
    top.DetailGeometry.IsProjectionLocked = True
    top.CommitViewportChanges()
    top.CommitChanges()
    plan.SetPageAsActive()

    view = doc.Views.AddPageView("View", 210, 297)
    persp = view.AddDetailView("Perspective detail", rg.Point2d(15, 100), rg.Point2d(195, 280), Rhino.Display.DefinedViewportProjection.Perspective)
    view.SetActiveDetail(persp.Id)
    persp.Viewport.SetCameraLocations(rg.Point3d(5000, 3000, 1500), rg.Point3d(-15000, -20000, 12000))
    persp.CommitViewportChanges()
    persp.CommitChanges()
    view.SetPageAsActive()

    # on the first page: a title and a line under it
    a = Rhino.DocObjects.ObjectAttributes()
    a.Space = Rhino.DocObjects.ActiveSpace.PageSpace
    a.ViewportId = plan.MainViewport.Id
    a.Name = "Page text"
    doc.Objects.AddText("Plan 1:100", rg.Plane(rg.Point3d(240, 40, 0), rg.Vector3d.ZAxis), 5.0, "Arial", False, False, a)
    a.Name = "Page line"
    doc.Objects.AddLine(rg.Point3d(240, 35, 0), rg.Point3d(400, 35, 0), a)
    for d, nm in [(top, "Top detail"), (persp, "Perspective detail")]:
        at = d.Attributes.Duplicate()
        at.Name = nm
        doc.Objects.ModifyAttributes(d.Id, at, True)

    out = {"pages": [], "details": [], "page_objects": []}
    for p in doc.Views.GetPageViews():
        out["pages"].append({
            "name": p.PageName,
            "id": str(p.MainViewport.Id),
            "width": p.PageWidth,
            "height": p.PageHeight,
            "number": p.PageNumber,
        })
        for d in p.GetDetailViews():
            vp = d.Viewport
            fr = vp.GetFrustum()
            g = d.DetailGeometry
            bb = d.Geometry.GetBoundingBox(True)
            out["details"].append({
                "name": d.Attributes.Name,
                "page": str(p.MainViewport.Id),
                "corners": [xyz(bb.Min), xyz(bb.Max)],
                "perspective": vp.IsPerspectiveProjection,
                "location": xyz(vp.CameraLocation),
                "direction": xyz(vp.CameraDirection),
                "up": xyz(vp.CameraUp),
                "frustum": list(fr[1:]) if isinstance(fr, tuple) else [],
                "scale": g.PageToModelRatio,
                "locked": g.IsProjectionLocked,
                "viewport": str(vp.Id),
            })
    doc.WriteFile(os.path.join(OUT, "layouts.3dm"), Rhino.FileIO.FileWriteOptions())
    # what Rhino reads back from the file
    f = Rhino.FileIO.File3dm.Read(os.path.join(OUT, "layouts.3dm"))
    for ob in f.Objects:
        at = ob.Attributes
        if at.Space == Rhino.DocObjects.ActiveSpace.PageSpace:
            g = ob.Geometry
            item = {"name": at.Name, "page": str(at.ViewportId), "kind": g.GetType().Name}
            if isinstance(g, rg.DetailView):
                item["scale"] = g.PageToModelRatio
                item["locked"] = g.IsProjectionLocked
                item["perspective"] = g.IsPerspectiveProjection
            out["page_objects"].append(item)
    out["views"] = [{"name": v.Name, "id": str(v.Viewport.Id)} for v in f.Views]
    with open(os.path.join(OUT, "layouts.json"), "w") as fh:
        json.dump(out, fh, indent=1)


try:
    main()
except Exception:
    import traceback
    with open(os.path.join(OUT, "layouts_error.txt"), "w") as fh:
        fh.write(traceback.format_exc())
