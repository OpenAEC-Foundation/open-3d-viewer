# -*- coding: utf-8 -*-
"""Test models for the open-3d-viewer Rhino reader: every file is written WITHOUT render
meshes, so the viewer has to mesh the exact geometry itself. Run in Rhino with the environment
variable MODELS_OUT set to the output folder."""
import os
import math
import traceback
import System
import Rhino
import Rhino.Geometry as rg
import scriptcontext as sc

OUT = os.environ.get("MODELS_OUT") or os.path.join(os.path.expanduser("~"), "rhino-test-models")
if not os.path.isdir(OUT):
    os.makedirs(OUT)
TOL = sc.doc.ModelAbsoluteTolerance
ATOL = sc.doc.ModelAngleToleranceRadians
log = open(os.path.join(OUT, "log.txt"), "w")


def say(*a):
    log.write(" ".join(str(x) for x in a) + "\n")
    log.flush()


def clear():
    for o in list(sc.doc.Objects):
        sc.doc.Objects.Delete(o, True)
    for i in range(sc.doc.Layers.Count - 1, 0, -1):
        try:
            sc.doc.Layers.Delete(i, True)
        except Exception:
            pass


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
    try:
        opts.WriteGeometryOnly = False
    except Exception:
        pass
    path = os.path.join(OUT, name + ".3dm")
    ok = sc.doc.WriteFile(path, opts)
    say("saved", name, ok, sc.doc.Objects.Count, "objects")
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


def primitives():
    clear()
    L = layer("Primitives", (200, 80, 60))
    add(rg.Sphere(rg.Point3d(0, 0, 5), 5).ToBrep(), L, "Sphere")
    add(rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(15, 0, 0), rg.Vector3d.ZAxis), 4), 10).ToBrep(True, True), L, "Cylinder")
    add(rg.Cone(rg.Plane(rg.Point3d(30, 0, 10), -rg.Vector3d.ZAxis), 10, 4).ToBrep(True), L, "Cone")
    add(rg.Torus(rg.Plane(rg.Point3d(45, 0, 2), rg.Vector3d.ZAxis), 5, 2).ToRevSurface().ToBrep(), L, "Torus")
    add(rg.Box(rg.BoundingBox(rg.Point3d(55, -4, 0), rg.Point3d(65, 4, 8))).ToBrep(), L, "Box")
    # a plane surface and an untrimmed NURBS patch
    add(rg.PlaneSurface(rg.Plane.WorldXY, rg.Interval(-10, 70), rg.Interval(-15, -8)), layer("Ground", (90, 140, 70)), "Plane")
    pts = [rg.Point3d(x * 4, 12 + y * 4, 2 * math.sin(x) * math.cos(y)) for x in range(6) for y in range(5)]
    add(rg.NurbsSurface.CreateFromPoints(pts, 6, 5, 3, 3), layer("Freeform", (60, 120, 200)), "Wavy patch")
    save("primitives")


def booleans():
    clear()
    L = layer("Solids", (180, 180, 190))
    box = rg.Box(rg.BoundingBox(rg.Point3d(0, 0, 0), rg.Point3d(20, 12, 6))).ToBrep()
    cutters = []
    for i in range(3):
        c = rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(4 + i * 6, 6, -1), rg.Vector3d.ZAxis), 1.5 + 0.5 * i), 8).ToBrep(True, True)
        cutters.append(c)
    cutters.append(rg.Sphere(rg.Point3d(20, 12, 6), 4).ToBrep())
    plate = first(rg.Brep.CreateBooleanDifference([box], cutters, TOL))
    add(plate, L, "Plate with holes")
    # a union of a cylinder and a box: curved trims
    a = rg.Box(rg.BoundingBox(rg.Point3d(30, 0, 0), rg.Point3d(40, 10, 4))).ToBrep()
    b = rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(35, 5, 0), rg.Vector3d.ZAxis), 3), 12).ToBrep(True, True)
    add(first(rg.Brep.CreateBooleanUnion([a, b], TOL)), L, "Box and tower")
    # a pipe through a sphere: intersecting curved surfaces
    s = rg.Sphere(rg.Point3d(55, 5, 5), 5).ToBrep()
    p = rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(48, 5, 5), rg.Vector3d.XAxis), 2), 14).ToBrep(True, True)
    add(first(rg.Brep.CreateBooleanDifference([s], [p], TOL)), layer("Spheres", (220, 170, 40)), "Sphere with a tunnel")
    save("booleans")


def fillets():
    clear()
    L = layer("Fillets", (120, 90, 200))
    box = rg.Box(rg.BoundingBox(rg.Point3d(0, 0, 0), rg.Point3d(10, 8, 5))).ToBrep()
    edges = [e.EdgeIndex for e in box.Edges]
    radii = [1.0] * len(edges)
    try:
        f = first(rg.Brep.CreateFilletEdges(box, edges, radii, radii, rg.BlendType.Fillet, rg.RailType.RollingBall, TOL))
    except Exception:
        f = None
        say(traceback.format_exc())
    add(f or box, L, "Rounded box")
    # a pipe along a 3d curve with round caps, and a variable radius pipe
    crv = rg.Curve.CreateInterpolatedCurve([rg.Point3d(15, 0, 0), rg.Point3d(20, 5, 3), rg.Point3d(25, 0, 6), rg.Point3d(30, 6, 2)], 3)
    add(first(rg.Brep.CreatePipe(crv, 0.8, False, rg.PipeCapMode.Round, True, TOL, ATOL)), L, "Pipe")
    crv2 = rg.LineCurve(rg.Point3d(35, 0, 0), rg.Point3d(35, 0, 10))
    add(first(rg.Brep.CreatePipe(crv2, [0.0, 0.5, 1.0], [2.0, 0.5, 1.5], True, rg.PipeCapMode.Flat, True, TOL, ATOL)), L, "Variable pipe")
    save("fillets")


def extrusions():
    clear()
    L = layer("Extrusions", (70, 160, 160))
    # a round column, a rectangular tube (profile with a hole), an I-beam and an open profile
    add(rg.Extrusion.Create(rg.ArcCurve(rg.Circle(rg.Point3d(0, 0, 0), 1.5)), 8.0, True), L, "Column")
    outer = rg.Rectangle3d(rg.Plane(rg.Point3d(6, 0, 0), rg.Vector3d.ZAxis), rg.Interval(-2, 2), rg.Interval(-1.5, 1.5)).ToNurbsCurve()
    inner = rg.Rectangle3d(rg.Plane(rg.Point3d(6, 0, 0), rg.Vector3d.ZAxis), rg.Interval(-1.6, 1.6), rg.Interval(-1.1, 1.1)).ToNurbsCurve()
    tube = rg.Extrusion()
    tube.SetOuterProfile(outer, True)
    tube.AddInnerProfile(inner)
    tube.SetPathAndUp(rg.Point3d(6, 0, 0), rg.Point3d(6, 0, 10), rg.Vector3d.YAxis)
    add(tube, L, "Tube")
    ib = [(-1, -1.5), (1, -1.5), (1, -1.3), (0.1, -1.3), (0.1, 1.3), (1, 1.3), (1, 1.5), (-1, 1.5), (-1, 1.3), (-0.1, 1.3), (-0.1, -1.3), (-1, -1.3), (-1, -1.5)]
    pl = rg.PolylineCurve([rg.Point3d(12 + x, y, 0) for x, y in ib])
    add(rg.Extrusion.Create(pl, 9.0, True), L, "I-beam")
    wave = rg.Curve.CreateInterpolatedCurve([rg.Point3d(18 + i, math.sin(i), 0) for i in range(8)], 3)
    add(rg.Extrusion.Create(wave, 4.0, False), L, "Wave wall")
    # a horizontal extrusion: a beam along x
    beam = rg.Extrusion.Create(rg.Rectangle3d(rg.Plane(rg.Point3d(0, 6, 3), rg.Vector3d.XAxis), rg.Interval(-0.5, 0.5), rg.Interval(-0.3, 0.3)).ToNurbsCurve(), -25.0, True)
    add(beam, L, "Beam")
    save("extrusions")


def freeform():
    clear()
    L = layer("Freeform", (200, 120, 160))
    # loft through circles and ellipses
    sections = [rg.Ellipse(rg.Plane(rg.Point3d(0, 0, z), rg.Vector3d.ZAxis), 3 + math.sin(z), 2 + 0.5 * math.cos(z)).ToNurbsCurve() for z in (0, 2, 4, 6, 8)]
    add(first(rg.Brep.CreateFromLoft(sections, rg.Point3d.Unset, rg.Point3d.Unset, rg.LoftType.Normal, False)), L, "Vase")
    # a revolved profile (surface of revolution)
    prof = rg.Curve.CreateInterpolatedCurve([rg.Point3d(12 + r, 0, z) for r, z in ((2, 0), (3, 1), (1.5, 3), (2.5, 5), (0.5, 7))], 3)
    rev = rg.RevSurface.Create(prof, rg.Line(rg.Point3d(12, 0, 0), rg.Point3d(12, 0, 1)))
    add(rev, L, "Revolved")
    # a sweep along a rail
    rail = rg.Curve.CreateInterpolatedCurve([rg.Point3d(20, 0, 0), rg.Point3d(25, 4, 0), rg.Point3d(30, 0, 2)], 3)
    shape = rg.Circle(rg.Plane(rail.PointAtStart, rail.TangentAtStart), 0.7).ToNurbsCurve()
    add(first(rg.Brep.CreateFromSweep(rail, shape, True, TOL)), L, "Sweep")
    # a trimmed freeform surface: a patch cut by a cylinder
    pts = [rg.Point3d(35 + x * 3, y * 3, math.sin(x * 0.8) + math.cos(y * 0.7)) for x in range(5) for y in range(5)]
    srf = rg.NurbsSurface.CreateFromPoints(pts, 5, 5, 3, 3).ToBrep()
    cyl = rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(41, 6, -5), rg.Vector3d.ZAxis), 2.5), 12).ToBrep(True, True)
    cut = rg.Brep.CreateBooleanSplit(srf, cyl, TOL) if hasattr(rg.Brep, "CreateBooleanSplit") else None
    pieces = srf.Split(cyl, TOL)
    if pieces and len(pieces) > 1:
        # keep the largest piece: the patch with a hole
        pieces = sorted(pieces, key=lambda b: -rg.AreaMassProperties.Compute(b).Area)
        add(pieces[0], L, "Patch with a hole")
    else:
        add(srf, L, "Patch")
    # an edge surface (Coons patch) from four curves
    c1 = rg.Curve.CreateInterpolatedCurve([rg.Point3d(52, 0, 0), rg.Point3d(55, 0, 2), rg.Point3d(58, 0, 0)], 3)
    c2 = rg.Curve.CreateInterpolatedCurve([rg.Point3d(58, 0, 0), rg.Point3d(58, 3, 1), rg.Point3d(58, 6, 0)], 3)
    c3 = rg.Curve.CreateInterpolatedCurve([rg.Point3d(58, 6, 0), rg.Point3d(55, 6, -2), rg.Point3d(52, 6, 0)], 3)
    c4 = rg.Curve.CreateInterpolatedCurve([rg.Point3d(52, 6, 0), rg.Point3d(52, 3, 1), rg.Point3d(52, 0, 0)], 3)
    add(rg.Brep.CreateEdgeSurface([c1, c2, c3, c4]), L, "Edge surface")
    save("freeform")


def building():
    """A small building: walls with window openings, a slab, a pitched roof and columns."""
    clear()
    LW = layer("Walls", (230, 225, 210))
    LS = layer("Slabs", (150, 150, 150))
    LR = layer("Roof", (170, 60, 40))
    LG = layer("Glass", (120, 170, 220))
    LC = layer("Columns", (90, 90, 90))
    W, D, H, T = 16.0, 10.0, 3.0, 0.3
    walls = [
        rg.Box(rg.BoundingBox(rg.Point3d(0, 0, 0), rg.Point3d(W, T, H))).ToBrep(),
        rg.Box(rg.BoundingBox(rg.Point3d(0, D - T, 0), rg.Point3d(W, D, H))).ToBrep(),
        rg.Box(rg.BoundingBox(rg.Point3d(0, 0, 0), rg.Point3d(T, D, H))).ToBrep(),
        rg.Box(rg.BoundingBox(rg.Point3d(W - T, 0, 0), rg.Point3d(W, D, H))).ToBrep(),
    ]
    shell = first(rg.Brep.CreateBooleanUnion(walls, TOL))
    openings = []
    for i in range(4):
        x = 2 + i * 3.5
        openings.append(rg.Box(rg.BoundingBox(rg.Point3d(x, -1, 0.9), rg.Point3d(x + 1.6, 1, 2.4))).ToBrep())
        add(rg.Box(rg.BoundingBox(rg.Point3d(x, 0.12, 0.9), rg.Point3d(x + 1.6, 0.16, 2.4))).ToBrep(), LG, "Window %d" % (i + 1))
    openings.append(rg.Box(rg.BoundingBox(rg.Point3d(7, D - 1, 0), rg.Point3d(8.2, D + 1, 2.2))).ToBrep())
    arch = rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(-1, 5, 2.0), rg.Vector3d.XAxis), 0.8), 2).ToBrep(True, True)
    openings.append(arch)
    walls_cut = first(rg.Brep.CreateBooleanDifference([shell], openings, TOL)) if shell else None
    add(walls_cut or shell, LW, "Walls")
    add(rg.Box(rg.BoundingBox(rg.Point3d(-0.5, -0.5, -0.3), rg.Point3d(W + 0.5, D + 0.5, 0))).ToBrep(), LS, "Ground slab")
    # a gable roof as an extrusion of a triangle along the length
    tri = rg.PolylineCurve([rg.Point3d(-0.5, -0.8, H), rg.Point3d(-0.5, D / 2, H + 2.5), rg.Point3d(-0.5, D + 0.8, H), rg.Point3d(-0.5, -0.8, H)])
    roof = rg.Extrusion.Create(tri, -(W + 1.0), True)
    add(roof, LR, "Roof")
    for i in range(5):
        add(rg.Cylinder(rg.Circle(rg.Plane(rg.Point3d(i * 4, -2.5, 0), rg.Vector3d.ZAxis), 0.2), H).ToBrep(True, True), LC, "Column %d" % (i + 1))
    save("building")


for job in (primitives, booleans, fillets, extrusions, freeform, building):
    try:
        say("==", job.__name__)
        job()
    except Exception:
        say(traceback.format_exc())
sc.doc.Modified = False
say("done")
log.close()
