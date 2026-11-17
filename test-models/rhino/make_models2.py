# -*- coding: utf-8 -*-
"""Harder test models (written without render meshes) and, for every .3dm in MODELS_OUT, the
area of each object as Rhino computes it (areas.json), to check the viewer's meshes against."""
import os
import json
import math
import traceback
import System
import Rhino
import Rhino.Geometry as rg
import scriptcontext as sc

OUT = os.environ.get("MODELS_OUT")
TOL = sc.doc.ModelAbsoluteTolerance
ATOL = sc.doc.ModelAngleToleranceRadians
log = open(os.path.join(OUT, "log2.txt"), "w")


def say(*a):
    log.write(" ".join(str(x) for x in a) + "\n")
    log.flush()


def clear():
    ids = [o.Id for o in sc.doc.Objects]
    for i in ids:
        sc.doc.Objects.Delete(i, True)


def layer(name, color):
    idx = sc.doc.Layers.FindByFullPath(name, -1)
    if idx >= 0:
        return idx
    l = Rhino.DocObjects.Layer()
    l.Name = name
    l.Color = System.Drawing.Color.FromArgb(*color)
    return sc.doc.Layers.Add(l)


def add(geom, lay, name=None):
    if geom is None:
        say("  !! None for", name)
        return None
    a = Rhino.DocObjects.ObjectAttributes()
    a.LayerIndex = lay
    if name:
        a.Name = name
    if isinstance(geom, rg.Extrusion):
        return sc.doc.Objects.AddExtrusion(geom, a)
    if isinstance(geom, rg.Brep):
        return sc.doc.Objects.AddBrep(geom, a)
    if isinstance(geom, rg.Surface):
        return sc.doc.Objects.AddSurface(geom, a)
    raise TypeError(type(geom))


def first(breps):
    return breps[0] if breps and len(breps) else None


def save(name):
    opts = Rhino.FileIO.FileWriteOptions()
    try:
        opts.IncludeRenderMeshes = False
    except Exception:
        pass
    path = os.path.join(OUT, name + ".3dm")
    ok = sc.doc.WriteFile(path, opts)
    say("saved", name, ok, len(list(sc.doc.Objects)), "objects")
    view = sc.doc.Views.ActiveView
    if view:
        vp = view.ActiveViewport
        vp.ChangeToPerspectiveProjection(True, 50.0)
        DMD = Rhino.Display.DisplayModeDescription
        mode = DMD.GetDisplayMode(DMD.ShadedId)
        if mode:
            vp.DisplayMode = mode
        vp.ZoomExtents()
        view.Redraw()
        bmp = view.CaptureToBitmap(System.Drawing.Size(1200, 800), mode)
        if bmp:
            bmp.Save(os.path.join(OUT, name + ".png"))


def tower():
    """A twisted tower (loft of rotated squares), floor slabs cut by a core, a shell canopy."""
    clear()
    LT = layer("Tower", (110, 140, 190))
    LF = layer("Floors", (170, 170, 170))
    LC = layer("Canopy", (220, 200, 150))
    secs = []
    for k in range(9):
        z = k * 4.0
        pl = rg.Plane(rg.Point3d(0, 0, z), rg.Vector3d.ZAxis)
        pl.Rotate(math.radians(10 * k), rg.Vector3d.ZAxis)
        s = 8.0 - 0.3 * k
        secs.append(rg.Rectangle3d(pl, rg.Interval(-s, s), rg.Interval(-s, s)).ToNurbsCurve())
    skin = first(rg.Brep.CreateFromLoft(secs, rg.Point3d.Unset, rg.Point3d.Unset, rg.LoftType.Normal, False))
    if skin:
        skin = skin.CapPlanarHoles(TOL) or skin
    add(skin, LT, "Twisted tower")
    core = rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(0, 0, -1), rg.Vector3d.ZAxis), 2.5), 40).ToBrep(True, True)
    for k in range(1, 8):
        slab = rg.Box(rg.BoundingBox(rg.Point3d(-14, -14, k * 4.0), rg.Point3d(-9, 14, k * 4.0 + 0.3))).ToBrep()
        add(slab, LF, "Balcony %d" % k)
    disc = rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(0, 0, 0), rg.Vector3d.ZAxis), 6), 0.3).ToBrep(True, True)
    add(first(rg.Brep.CreateBooleanDifference([disc], [core], TOL)), LF, "Ring slab")
    # a doubly curved canopy, trimmed to an ellipse
    pts = []
    for i in range(7):
        for j in range(7):
            x, y = 20 + i * 3, -9 + j * 3
            pts.append(rg.Point3d(x, y, 6 + 1.5 * math.cos((i - 3) * 0.5) * math.cos((j - 3) * 0.5)))
    srf = rg.NurbsSurface.CreateFromPoints(pts, 7, 7, 3, 3)
    ell = rg.Ellipse(rg.Plane(rg.Point3d(29, 0, 0), rg.Vector3d.ZAxis), 8, 6).ToNurbsCurve()
    proj = rg.Curve.ProjectToBrep(ell, srf.ToBrep(), rg.Vector3d.ZAxis, TOL)
    canopy = None
    if proj and len(proj):
        faces = srf.ToBrep().Faces
        split = faces[0].Split(proj, TOL)
        if split and split.Faces.Count > 1:
            # the face whose centre is near the ellipse centre
            best = None
            for f in split.Faces:
                c = rg.AreaMassProperties.Compute(f.DuplicateFace(False)).Centroid
                d = c.DistanceTo(rg.Point3d(29, 0, c.Z))
                if best is None or d < best[0]:
                    best = (d, f.DuplicateFace(False))
            canopy = best[1]
    add(canopy or srf, LC, "Canopy")
    save("tower")


def mechanical():
    """A flange: revolved profile, bolt holes, fillets, a chamfered hub; a gear-like extrusion."""
    clear()
    LM = layer("Steel", (150, 160, 170))
    prof = rg.PolylineCurve([rg.Point3d(0, 0, 0), rg.Point3d(10, 0, 0), rg.Point3d(10, 0, 1.5), rg.Point3d(4, 0, 1.5), rg.Point3d(3, 0, 5), rg.Point3d(1.5, 0, 5), rg.Point3d(1.5, 0, 0)])
    rev = rg.RevSurface.Create(prof, rg.Line(rg.Point3d(0, 0, 0), rg.Point3d(0, 0, 1)))
    flange = rev.ToBrep() if rev else None
    if flange:
        flange = flange.CapPlanarHoles(TOL) or flange
        holes = []
        for k in range(8):
            a = k * math.pi / 4
            holes.append(rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(7.5 * math.cos(a), 7.5 * math.sin(a), -1), rg.Vector3d.ZAxis), 0.7), 4).ToBrep(True, True))
        cut = first(rg.Brep.CreateBooleanDifference([flange], holes, TOL))
        add(cut or flange, LM, "Flange")
    # a gear: a polyline profile with 24 teeth, extruded, with a bore
    teeth = 24
    pts = []
    for k in range(teeth * 4):
        a = 2 * math.pi * k / (teeth * 4)
        r = 6.0 if (k % 4) in (1, 2) else 5.3
        pts.append(rg.Point3d(25 + r * math.cos(a), r * math.sin(a), 0))
    pts.append(pts[0])
    gear = rg.Extrusion()
    gear.SetOuterProfile(rg.PolylineCurve(pts), True)
    gear.AddInnerProfile(rg.ArcCurve(rg.Circle(rg.Point3d(25, 0, 0), 1.5)))
    gear.SetPathAndUp(rg.Point3d(25, 0, 0), rg.Point3d(25, 0, 2), rg.Vector3d.YAxis)
    add(gear, LM, "Gear")
    # a filleted L-bracket
    l = rg.PolylineCurve([rg.Point3d(40, 0, 0), rg.Point3d(48, 0, 0), rg.Point3d(48, 0, 1), rg.Point3d(41, 0, 1), rg.Point3d(41, 0, 8), rg.Point3d(40, 0, 8), rg.Point3d(40, 0, 0)])
    bracket = rg.Extrusion.Create(l, -6, True)
    b = bracket.ToBrep() if bracket else None
    if b:
        inner = [e.EdgeIndex for e in b.Edges if abs(e.PointAtStart.X - 41) < 1e-6 and abs(e.PointAtEnd.X - 41) < 1e-6 and abs(e.PointAtStart.Z - 1) < 1e-6 and abs(e.PointAtEnd.Z - 1) < 1e-6]
        f = None
        if inner:
            try:
                f = first(rg.Brep.CreateFilletEdges(b, inner, [0.8] * len(inner), [0.8] * len(inner), rg.BlendType.Fillet, rg.RailType.RollingBall, TOL))
            except Exception:
                say(traceback.format_exc())
        add(f or b, LM, "Bracket")
    save("mechanical")


def stairs():
    """A spiral stair (blocks rotated about a column), a handrail sweep and a ramp."""
    clear()
    LS = layer("Stair", (160, 120, 80))
    LR = layer("Rail", (60, 60, 60))
    add(rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(0, 0, 0), rg.Vector3d.ZAxis), 0.3), 6).ToBrep(True, True), LR, "Column")
    for k in range(20):
        step = rg.Box(rg.BoundingBox(rg.Point3d(0.2, -0.4, 0), rg.Point3d(2.2, 0.4, 0.06))).ToBrep()
        step.Rotate(math.radians(18 * k), rg.Vector3d.ZAxis, rg.Point3d.Origin)
        step.Translate(rg.Vector3d(0, 0, 0.28 * k))
        add(step, LS, "Step %d" % (k + 1))
    helix = rg.NurbsCurve.CreateSpiral(rg.Point3d(0, 0, 1.0), rg.Vector3d.ZAxis, rg.Point3d(2.1, 0, 1.0), 5.6 / 20 * 20, 1.0, 2.1, 2.1)
    if helix:
        add(first(rg.Brep.CreatePipe(helix, 0.04, False, rg.PipeCapMode.Round, True, TOL, ATOL)), LR, "Handrail")
    save("stairs")


def areas():
    out = {}
    for fn in sorted(os.listdir(OUT)):
        if not fn.endswith(".3dm"):
            continue
        f = Rhino.FileIO.File3dm.Read(os.path.join(OUT, fn))
        rows = []
        for o in f.Objects:
            g = o.Geometry
            if isinstance(g, rg.Extrusion):
                g = g.ToBrep()
            if not isinstance(g, rg.Brep):
                continue
            amp = rg.AreaMassProperties.Compute(g)
            rows.append({"name": o.Attributes.Name or "", "area": amp.Area if amp else None, "faces": g.Faces.Count})
        out[fn] = rows
    with open(os.path.join(OUT, "areas.json"), "w") as fh:
        json.dump(out, fh, indent=1)


for job in (tower, mechanical, stairs, areas):
    try:
        say("==", job.__name__)
        job()
    except Exception:
        say(traceback.format_exc())
sc.doc.Modified = False
say("done")
log.close()
