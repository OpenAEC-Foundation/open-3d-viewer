# -*- coding: utf-8 -*-
"""Third set of test models, written without render meshes: blocks of polysurfaces (nested and
mirrored), a dome with openings, a saddle roof, a SubD turned NURBS and a large facade."""
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
log = open(os.path.join(OUT, "log3.txt"), "w")


def say(*a):
    log.write(" ".join(str(x) for x in a) + "\n")
    log.flush()


def clear():
    for i in [o.Id for o in sc.doc.Objects]:
        sc.doc.Objects.Delete(i, True)


def layer(name, color):
    idx = sc.doc.Layers.FindByFullPath(name, -1)
    if idx >= 0:
        return idx
    l = Rhino.DocObjects.Layer()
    l.Name = name
    l.Color = System.Drawing.Color.FromArgb(*color)
    return sc.doc.Layers.Add(l)


def attrs(lay, name=None):
    a = Rhino.DocObjects.ObjectAttributes()
    a.LayerIndex = lay
    if name:
        a.Name = name
    return a


def add(geom, lay, name=None):
    if geom is None:
        say("  !! None for", name)
        return None
    a = attrs(lay, name)
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
    ok = sc.doc.WriteFile(os.path.join(OUT, name + ".3dm"), opts)
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


def blocks():
    """A chair (polysurfaces) as a block, a table block holding four chairs (nested), placed,
    rotated and mirrored."""
    clear()
    LB = layer("Furniture", (140, 100, 60))
    seat = rg.Box(rg.BoundingBox(rg.Point3d(-0.25, -0.25, 0.42), rg.Point3d(0.25, 0.25, 0.47))).ToBrep()
    back = rg.Box(rg.BoundingBox(rg.Point3d(-0.25, 0.2, 0.47), rg.Point3d(0.25, 0.25, 0.95))).ToBrep()
    legs = [rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(x, y, 0), rg.Vector3d.ZAxis), 0.02), 0.42).ToBrep(True, True) for x in (-0.22, 0.22) for y in (-0.22, 0.22)]
    chair = sc.doc.InstanceDefinitions.Add("Chair", "", rg.Point3d.Origin, [seat, back] + legs, [attrs(LB)] * 6)
    top = rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(0, 0, 0.72), rg.Vector3d.ZAxis), 0.6), 0.04).ToBrep(True, True)
    foot = rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(0, 0, 0), rg.Vector3d.ZAxis), 0.05), 0.72).ToBrep(True, True)
    table_geo = [top, foot]
    table_attr = [attrs(LB), attrs(LB)]
    # four chairs as nested instances
    insts = []
    for k in range(4):
        a = k * math.pi / 2
        xf = rg.Transform.Translation(rg.Vector3d(0.9 * math.cos(a), 0.9 * math.sin(a), 0)) * rg.Transform.Rotation(a + math.pi / 2, rg.Point3d.Origin)
        insts.append(rg.InstanceReferenceGeometry(sc.doc.InstanceDefinitions[chair].Id, xf))
    table = sc.doc.InstanceDefinitions.Add("Table set", "", rg.Point3d.Origin, table_geo + insts, table_attr + [attrs(LB)] * 4)
    for i in range(3):
        sc.doc.Objects.AddInstanceObject(table, rg.Transform.Translation(rg.Vector3d(i * 3.0, 0, 0)), attrs(LB, "Table %d" % (i + 1)))
    mirror = rg.Transform.Mirror(rg.Plane(rg.Point3d(0, -2, 0), rg.Vector3d.YAxis))
    sc.doc.Objects.AddInstanceObject(chair, mirror * rg.Transform.Translation(rg.Vector3d(1, 0, 0)), attrs(LB, "Mirrored chair"))
    save("blocks")


def dome():
    """A hemispherical dome with four arched openings and an oculus, on a drum."""
    clear()
    LD = layer("Dome", (220, 210, 190))
    sphere = rg.Sphere(rg.Point3d(0, 0, 4), 6).ToBrep()
    cutter = rg.Box(rg.BoundingBox(rg.Point3d(-7, -7, -10), rg.Point3d(7, 7, 4))).ToBrep()
    half = first(rg.Brep.CreateBooleanDifference([sphere], [cutter], TOL))
    shell = None
    if half:
        inner = rg.Sphere(rg.Point3d(0, 0, 4), 5.6).ToBrep()
        shell = first(rg.Brep.CreateBooleanDifference([half], [inner], TOL)) or half
        cuts = [rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(0, 0, 9), rg.Vector3d.ZAxis), 1.0), 3).ToBrep(True, True)]
        for k in range(4):
            a = k * math.pi / 2 + math.pi / 4
            pl = rg.Plane(rg.Point3d(0, 0, 6), rg.Vector3d(math.cos(a), math.sin(a), 0))
            cuts.append(rg.Cylinder(rg.Circle(pl, 1.2), 8).ToBrep(True, True))
        shell = first(rg.Brep.CreateBooleanDifference([shell], cuts, TOL)) or shell
    add(shell, LD, "Dome")
    drum = rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(0, 0, 0), rg.Vector3d.ZAxis), 6), 4).ToBrep(True, True)
    hole = rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(0, 0, -1), rg.Vector3d.ZAxis), 5.6), 6).ToBrep(True, True)
    add(first(rg.Brep.CreateBooleanDifference([drum], [hole], TOL)), LD, "Drum")
    save("dome")


def roofs():
    """A saddle (hyperbolic paraboloid) roof, a SubD blob as NURBS and a twisted ribbon."""
    clear()
    LR = layer("Roofs", (90, 130, 180))
    pts = [rg.Point3d(0, 0, 3), rg.Point3d(12, 0, 7), rg.Point3d(0, 10, 7), rg.Point3d(12, 10, 3)]
    hypar = rg.NurbsSurface.CreateFromCorners(pts[0], pts[1], pts[3], pts[2])
    add(hypar, LR, "Hypar")
    LB = layer("Blob", (200, 100, 140))
    try:
        mesh = rg.Mesh.CreateFromBox(rg.Box(rg.BoundingBox(rg.Point3d(18, 0, 0), rg.Point3d(24, 5, 4))), 2, 2, 2)
        subd = rg.SubD.CreateFromMesh(mesh)
        blob = subd.ToBrep(rg.SubDToBrepOptions.Default) if subd else None
        add(blob, LB, "SubD blob")
    except Exception:
        say(traceback.format_exc())
    rails = []
    for k in range(2):
        rails.append(rg.Curve.CreateInterpolatedCurve([rg.Point3d(30 + i, (1 if k else -1) * math.cos(i * 0.4), (1 if k else -1) * math.sin(i * 0.4)) for i in range(16)], 3))
    add(first(rg.Brep.CreateFromLoft(rails, rg.Point3d.Unset, rg.Point3d.Unset, rg.LoftType.Normal, False)), LR, "Ribbon")
    save("roofs")


def facade():
    """A facade of 40 by 12 panels (extrusions and polysurfaces with openings): many objects."""
    clear()
    LP = layer("Panels", (180, 180, 170))
    LG = layer("Glass", (130, 180, 220))
    panel = rg.Box(rg.BoundingBox(rg.Point3d(0, 0, 0), rg.Point3d(1.4, 0.2, 3.3))).ToBrep()
    window = rg.Box(rg.BoundingBox(rg.Point3d(0.3, -1, 0.9), rg.Point3d(1.1, 1, 2.6))).ToBrep()
    holed = first(rg.Brep.CreateBooleanDifference([panel], [window], TOL)) or panel
    glass = rg.Box(rg.BoundingBox(rg.Point3d(0.3, 0.09, 0.9), rg.Point3d(1.1, 0.11, 2.6))).ToBrep()
    for i in range(40):
        for j in range(12):
            xf = rg.Transform.Translation(rg.Vector3d(i * 1.5, 0, j * 3.4))
            p = holed.DuplicateBrep()
            p.Transform(xf)
            add(p, LP, "Panel %d.%d" % (i, j))
            g = glass.DuplicateBrep()
            g.Transform(xf)
            add(g, LG)
    save("facade")


def areas():
    out = {}
    for fn in ("blocks.3dm", "dome.3dm", "roofs.3dm", "facade.3dm"):
        path = os.path.join(OUT, fn)
        if not os.path.exists(path):
            continue
        f = Rhino.FileIO.File3dm.Read(path)
        rows = []
        for o in f.Objects:
            g = o.Geometry
            if isinstance(g, rg.Extrusion):
                g = g.ToBrep()
            if isinstance(g, rg.Surface):
                g = g.ToBrep()
            if not isinstance(g, rg.Brep):
                continue
            amp = rg.AreaMassProperties.Compute(g)
            rows.append({"name": o.Attributes.Name or "", "area": amp.Area if amp else None, "mode": str(o.Attributes.Mode)})
        out[fn] = rows
    with open(os.path.join(OUT, "areas3.json"), "w") as fh:
        json.dump(out, fh, indent=1)


for job in (blocks, dome, roofs, facade, areas):
    try:
        say("==", job.__name__)
        job()
    except Exception:
        say(traceback.format_exc())
sc.doc.Modified = False
say("done")
log.close()
